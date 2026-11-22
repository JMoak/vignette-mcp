"use strict";

// Turns what bench_probe saw (every change of every window's position, timestamped on the
// performance counter, plus the compositor's vertical blank clock) into the bench's measures.

function analyse(probeText, durationMs) {
  const probe = parseProbe(probeText);
  const windows = probe.origins.length;
  const marked = betweenMarks(probe);
  if (probe.moves.length === 0) return { windows, landings: 0, marked };
  const clock = refreshClock(probe);
  const started = probe.moves[0].at;
  const fastUntil = started + (durationMs / 2 / 1000) * probe.hz;
  const fast = probe.moves.filter((move) => move.at < fastUntil);
  return {
    windows,
    landings: probe.moves.length,
    refreshHz: round(probe.hz / clock.period, 3),
    updatesPerSecond: spreadOf(countsByWindow(probe.moves, windows).map((count) => count / (durationMs / 1000))),
    updatesPerSecondFastHalf: paceByWindow(fast, windows, probe.hz),
    refreshesInFastHalf: refreshUse(fast, windows, clock),
    longestWaitForNextPositionMs: longestWait(fast, windows, probe.hz),
    together: togetherness(probe, clock),
    lastLandingAfterSecondMarkMs: marked ? round(((probe.moves[probe.moves.length - 1].at - probe.marks[1].at) / probe.hz) * 1000, 3) : null,
    marked,
    pollGapMs: round((probe.longestGap / probe.hz) * 1000, 3),
    polls: probe.polls,
    cyclesPerSecond: probe.cyclesPerSecond,
  };
}

function parseProbe(text) {
  const probe = { hz: 0, cyclesPerSecond: 0, vblanks: [], origins: [], moves: [], marks: [], polls: 0, longestGap: 0 };
  for (const line of text.split(/\r?\n/)) {
    const [kind, ...fields] = line.trim().split(" ");
    const numbers = fields.map(Number);
    if (kind === "clock") probe.hz = numbers[0];
    if (kind === "cycles_per_second") probe.cyclesPerSecond = numbers[0];
    if (kind === "vblank") probe.vblanks.push({ refresh: numbers[0], at: numbers[1], period: numbers[2] });
    if (kind === "origin") probe.origins[numbers[0]] = { x: numbers[1], y: numbers[2] };
    if (kind === "move") probe.moves.push({ at: numbers[0], index: numbers[1], x: numbers[2], y: numbers[3] });
    if (kind === "mark") probe.marks.push({ at: numbers[0], cycles: numbers.slice(1) });
    if (kind === "polls") [probe.polls, probe.longestGap] = numbers;
  }
  return probe;
}

function betweenMarks(probe) {
  if (probe.marks.length !== 2 || probe.cyclesPerSecond === 0) return null;
  const [from, to] = probe.marks;
  return {
    wallMs: round(((to.at - from.at) / probe.hz) * 1000, 3),
    cpuMs: to.cycles.map((cycles, index) => round(((cycles - from.cycles[index]) / probe.cyclesPerSecond) * 1000, 3)),
  };
}

function refreshClock(probe) {
  const [before, after] = probe.vblanks;
  const refreshes = after.refresh - before.refresh;
  const measured = refreshes > 0 ? (after.at - before.at) / refreshes : before.period;
  const period = Math.abs(measured - before.period) < before.period * 0.01 ? measured : before.period;
  const refreshOf = (at) => Math.floor((at - before.at) / period);
  const phaseMs = (at) => (((at - before.at) / period - refreshOf(at)) * period * 1000) / probe.hz;
  return { period, refreshOf, phaseMs };
}

function paceByWindow(moves, windows, hz) {
  const landings = Array.from({ length: windows }, () => []);
  for (const move of moves) landings[move.index].push(move.at);
  const paced = landings.filter((times) => times.length > 1);
  return spreadOf(paced.map((times) => ((times.length - 1) * hz) / (times[times.length - 1] - times[0])));
}

function longestWait(moves, windows, hz) {
  const previous = new Array(windows).fill(null);
  const longest = new Array(windows).fill(0);
  for (const move of moves) {
    if (previous[move.index] !== null) longest[move.index] = Math.max(longest[move.index], move.at - previous[move.index]);
    previous[move.index] = move.at;
  }
  return round((Math.max(...longest) / hz) * 1000, 3);
}

function refreshUse(moves, windows, clock) {
  if (moves.length === 0) return null;
  const first = clock.refreshOf(moves[0].at);
  const last = clock.refreshOf(moves[moves.length - 1].at);
  const perRefresh = new Map();
  for (const move of moves) {
    const key = `${move.index}:${clock.refreshOf(move.at)}`;
    perRefresh.set(key, (perRefresh.get(key) ?? 0) + 1);
  }
  const slots = (last - first + 1) * windows;
  const counts = [...perRefresh.values()];
  const doubled = counts.filter((count) => count > 1).length;
  return {
    refreshes: last - first + 1,
    withNewPosition: round(counts.length / slots, 4),
    withNoNewPosition: round((slots - counts.length) / slots, 4),
    withTwoOrMore: round(doubled / slots, 4),
  };
}

function togetherness(probe, clock) {
  const frames = framesByOffset(probe);
  const whole = frames.filter((frame) => frame.length === probe.origins.length);
  const spreads = whole.map((frame) => ((Math.max(...frame) - Math.min(...frame)) / probe.hz) * 1000);
  const sameRefresh = whole.filter((frame) => new Set(frame.map(clock.refreshOf)).size === 1);
  return {
    frames: frames.length,
    wholeFrames: whole.length,
    landedInOneRefresh: sameRefresh.length,
    landedInOneRefreshShare: round(sameRefresh.length / Math.max(1, whole.length), 4),
    spreadMs: spreadOf(spreads),
    firstLandingAfterVblankMs: spreadOf(whole.map((frame) => clock.phaseMs(Math.min(...frame)))),
    lastLandingAfterVblankMs: spreadOf(whole.map((frame) => clock.phaseMs(Math.max(...frame)))),
  };
}

function framesByOffset(probe) {
  const frames = new Map();
  for (const move of probe.moves) {
    const origin = probe.origins[move.index];
    const key = `${move.x - origin.x},${move.y - origin.y}`;
    if (!frames.has(key)) frames.set(key, new Map());
    if (!frames.get(key).has(move.index)) frames.get(key).set(move.index, move.at);
  }
  return [...frames.values()].map((frame) => [...frame.values()]);
}

function countsByWindow(moves, windows) {
  const counts = new Array(windows).fill(0);
  for (const move of moves) counts[move.index] += 1;
  return counts;
}

function spreadOf(values) {
  if (values.length === 0) return null;
  const sorted = [...values].sort((a, b) => a - b);
  const at = (share) => sorted[Math.min(sorted.length - 1, Math.floor(share * sorted.length))];
  return {
    min: round(sorted[0], 3),
    median: round(at(0.5), 3),
    p95: round(at(0.95), 3),
    max: round(sorted[sorted.length - 1], 3),
  };
}

function round(value, places) {
  return Number(value.toFixed(places));
}

module.exports = { analyse, parseProbe, spreadOf, round };
