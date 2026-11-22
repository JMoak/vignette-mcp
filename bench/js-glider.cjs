"use strict";

// A line-for-line port of packages/desktop/src/main/surfaces/pacer.ts and glide.ts from the
// Electron app, with the types removed. It is the baseline the helper is measured against.

const glideFrameMs = 8;

function framePacer(clock, pacing) {
  return (tick) => {
    let live = true;
    let due = clock.now() + pacing.frameMs;

    const waitUntilDue = () => {
      const left = due - clock.now();
      if (left > pacing.timerLatenessMs) clock.sleep(wake, left - pacing.timerLatenessMs);
      else clock.defer(wake);
    };

    const wake = () => {
      if (!live) return;
      if (clock.now() >= due) {
        due = clock.now() + pacing.frameMs;
        tick();
      }
      if (live) waitUntilDue();
    };

    waitUntilDue();
    return () => {
      live = false;
    };
  };
}

function timerLatenessOn(platform) {
  return platform === "win32" ? 16 : 1;
}

function easedToRest(progress) {
  return 1 - (1 - Math.min(1, Math.max(0, progress))) ** 5;
}

function glider(clock) {
  const runs = new Set();
  let stopTicking;

  const finish = (run) => {
    if (!runs.delete(run)) return;
    run.settled();
    if (runs.size > 0) return;
    stopTicking?.();
    stopTicking = undefined;
  };

  const drop = (key) => {
    for (const run of [...runs]) {
      if (run.legs.delete(key) && run.legs.size === 0) finish(run);
    }
  };

  const advance = (run, now) => {
    const progress = (now - run.startedAt) / run.durationMs;
    for (const leg of [...run.legs.values()]) step(leg, pointAlong(leg, easedToRest(progress)));
    if (progress >= 1) finish(run);
  };

  const tick = () => {
    const now = clock.now();
    for (const run of [...runs]) advance(run, now);
  };

  return {
    run: (tracks, durationMs) => {
      for (const track of tracks) drop(track.key);
      if (!(durationMs > 0)) return jumped(tracks);
      const legs = new Map(tracks.map((track) => [track.key, legOf(track)]));
      const moving = new Map([...legs].filter(([, leg]) => !samePoint(leg.from, leg.to)));
      if (moving.size === 0) return Promise.resolve();
      return new Promise((settled) => {
        runs.add({ startedAt: clock.now(), durationMs, legs: moving, settled });
        stopTicking ??= clock.everyFrame(tick);
      });
    },
    gliding: (key) => [...runs].some((run) => run.legs.has(key)),
    drop,
    stop: () => {
      for (const run of [...runs]) finish(run);
    },
  };
}

function jumped(tracks) {
  for (const track of tracks) track.body.moveTo(track.to);
  return Promise.resolve();
}

function legOf(track) {
  const from = track.body.position();
  return { body: track.body, from, to: track.to, at: from };
}

function pointAlong(leg, eased) {
  return {
    x: leg.from.x + Math.round((leg.to.x - leg.from.x) * eased),
    y: leg.from.y + Math.round((leg.to.y - leg.from.y) * eased),
  };
}

function step(leg, point) {
  if (samePoint(point, leg.at)) return;
  leg.at = point;
  leg.body.moveTo(point);
}

function samePoint(a, b) {
  return a.x === b.x && a.y === b.y;
}

module.exports = { glideFrameMs, framePacer, timerLatenessOn, easedToRest, glider };
