"use strict";

// Runs under the Electron binary in Node mode:
//   $env:ELECTRON_RUN_AS_NODE = "1"; & <electron.exe> bench\run.cjs [launches per way] [stall ms]
// Launches glide.cjs once per way of gliding per run, then once more for the border check, and
// prints the measures side by side. Every launch lasts a few seconds. With a stall, the Electron
// main thread is kept busy for that long once in each glide.

const { spawnSync } = require("node:child_process");
const fs = require("node:fs");
const path = require("node:path");
const { spreadOf, round } = require("./analyse.cjs");

const root = path.resolve(__dirname, "..");
const temp = path.join(__dirname, ".tmp");
const runs = Number(process.argv[2] ?? 3);
const stallMs = Number(process.argv[3] ?? 0);
const ways = ["js", "helper-async", "helper-defer"];
const built = ["target/release/kw-panes-mover.exe", "target/release/examples/bench_probe.exe"];
const launchLimitMs = 45_000;

main();

function main() {
  const missing = built.filter((file) => !fs.existsSync(path.join(root, file)));
  if (missing.length > 0) return fail(`build first: cargo build --release; cargo build --release --examples (missing ${missing.join(", ")})`);
  fs.mkdirSync(temp, { recursive: true });
  const launches = Object.fromEntries(ways.map((way) => [way, []]));
  for (let run = 1; run <= runs; run++) {
    for (const way of ways) launches[way].push(launch(way, run));
  }
  const border = launch("border", 1);
  const summary = {
    runs,
    stallMs,
    machine: border.machine,
    geometry: border.geometry,
    ways: Object.fromEntries(ways.map((way) => [way, summarise(launches[way])])),
    border: borderVerdict(border),
    leftBehind: leftBehind(),
  };
  const file = stallMs > 0 ? `summary-stall-${stallMs}.json` : "summary.json";
  fs.writeFileSync(path.join(temp, file), `${JSON.stringify(summary, null, 2)}\n`);
  print(summary);
}

function launch(mode, run) {
  const out = path.join(temp, `${mode}-${run}.json`);
  fs.rmSync(out, { force: true });
  const env = { ...process.env, KW_BENCH_MODE: mode, KW_BENCH_OUT: out, KW_BENCH_STALL_MS: String(stallMs) };
  delete env.ELECTRON_RUN_AS_NODE;
  const started = Date.now();
  const child = spawnSync(process.execPath, [path.join(__dirname, "glide.cjs")], { env, timeout: launchLimitMs, stdio: "ignore" });
  const result = fs.existsSync(out) ? JSON.parse(fs.readFileSync(out, "utf8")) : { mode, error: "no result written" };
  return { ...result, exitCode: child.status, launchMs: Date.now() - started };
}

function summarise(launches) {
  const glides = launches.flatMap((launch) => launch.glides ?? []);
  const each = (pick) => glides.map(pick).filter((value) => value !== null && value !== undefined);
  const together = each((glide) => glide.observed.together);
  return {
    launches: launches.length,
    failedLaunches: launches.filter(failed).map((launch) => launch.error ?? `exit ${launch.exitCode}`),
    launchSeconds: spreadOf(launches.map((launch) => launch.launchMs / 1000)),
    glides: glides.length,
    updatesPerSecondPerWindow: spreadOf(each((glide) => glide.observed.updatesPerSecondFastHalf?.median)),
    updatesPerSecondPerWindowWholeGlide: spreadOf(each((glide) => glide.observed.updatesPerSecond?.median)),
    refreshesWithNewPosition: spreadOf(each((glide) => glide.observed.refreshesInFastHalf?.withNewPosition)),
    refreshesWithTwoOrMore: spreadOf(each((glide) => glide.observed.refreshesInFastHalf?.withTwoOrMore)),
    longestWaitForNextPositionMs: spreadOf(each((glide) => glide.observed.longestWaitForNextPositionMs)),
    electronMainCpuShare: spreadOf(each((glide) => glide.cpuMs?.electronMainShareOfOneCore)),
    electronMainCpuMs: spreadOf(each((glide) => glide.cpuMs?.electronMain)),
    electronMainCpuShareByTicks: spreadOf(each((glide) => glide.electronMainByTickAccounting.shareOfOneCore)),
    helperCpuShare: spreadOf(each((glide) => glide.cpuMs?.helperShareOfOneCore)),
    helperCpuMs: spreadOf(each((glide) => glide.cpuMs?.helper)),
    electronRenderersCpuMs: spreadOf(each((glide) => glide.cpuMs?.electronRenderers)),
    electronGpuCpuMs: spreadOf(each((glide) => glide.cpuMs?.electronGpu)),
    issueMeanMs: spreadOf(each((glide) => glide.issue.meanMs)),
    issueWorstMs: spreadOf(each((glide) => glide.issue.worstMs)),
    framesIssued: spreadOf(each((glide) => glide.issue.frames)),
    missedRefreshes: each((glide) => glide.missedRefreshes).reduce((total, missed) => total + missed, 0),
    spreadMedianMs: spreadOf(together.map((entry) => entry.spreadMs.median)),
    spreadWorstMs: spreadOf(together.map((entry) => entry.spreadMs.max)),
    lastLandingAfterVblankWorstMs: spreadOf(together.map((entry) => entry.lastLandingAfterVblankMs.max)),
    framesLandedInOneRefresh: `${sum(together, "landedInOneRefresh")}/${sum(together, "wholeFrames")}`,
    framesLandedInOneRefreshShare: round(sum(together, "landedInOneRefresh") / Math.max(1, sum(together, "wholeFrames")), 4),
    framesSomeWindowSkipped: sum(together, "frames") - sum(together, "wholeFrames"),
    landedOnThePixel: `${glides.filter((glide) => glide.landedOnThePixel).length}/${glides.length}`,
    panesExposed: spreadOf(each((glide) => glide.panesExposed)),
    probeWorstPollGapMs: Math.max(0, ...each((glide) => glide.observed.pollGapMs)),
    helperExits: launches.map((launch) => launch.helperExit).filter(Boolean).map(exitText),
  };
}

function failed(launch) {
  return Boolean(launch.error) || launch.exitCode !== 0;
}

function exitText(exit) {
  return `code ${exit.code} ${exit.msAfterInputClosed} ms after its input closed`;
}

function borderVerdict(launch) {
  if (failed(launch) || !launch.border) return { error: launch.error ?? `exit ${launch.exitCode}` };
  const { border } = launch;
  return {
    colour: border.colour,
    refusedByHelper: border.refused.length,
    readBack: border.readBack.join("; "),
    windowsExposed: border.windowsExposed,
    edgePixelTookColourThenReverted: border.edgePixelTookColourThenReverted,
    edgeOfFirstPane: border.edgeOfFirstPane,
    launchSeconds: launch.launchMs / 1000,
    helperExit: exitText(launch.helperExit),
  };
}

function leftBehind() {
  return ["kw-panes-mover.exe", "bench_probe.exe"].filter((image) => {
    const listed = spawnSync("tasklist", ["/FI", `IMAGENAME eq ${image}`, "/FO", "CSV", "/NH"], { encoding: "utf8" });
    return (listed.stdout ?? "").toLowerCase().includes(image);
  });
}

function sum(entries, field) {
  return entries.reduce((total, entry) => total + entry[field], 0);
}

function print(summary) {
  const measured = ways.map((way) => summary.ways[way]);
  const display = summary.machine?.display;
  const rows = [
    ["glides measured", (way) => way.glides],
    ["position updates/s per window, first half of glide", (way) => range(way.updatesPerSecondPerWindow)],
    ["position updates/s per window, whole glide", (way) => range(way.updatesPerSecondPerWindowWholeGlide)],
    ["share of refreshes showing a new position", (way) => range(way.refreshesWithNewPosition, percent)],
    ["share of refreshes given two or more positions", (way) => range(way.refreshesWithTwoOrMore, percent)],
    ["longest wait for a window's next position, ms", (way) => range(way.longestWaitForNextPositionMs)],
    ["Electron main CPU, share of one core", (way) => range(way.electronMainCpuShare, percent)],
    ["Electron main CPU per glide, ms", (way) => range(way.electronMainCpuMs)],
    ["helper CPU, share of one core", (way) => range(way.helperCpuShare, percent)],
    ["helper CPU per glide, ms", (way) => range(way.helperCpuMs)],
    ["renderers CPU per glide, ms (16 processes)", (way) => range(way.electronRenderersCpuMs)],
    ["GPU process CPU per glide, ms", (way) => range(way.electronGpuCpuMs)],
    ["issuing one frame's 16 moves, mean ms", (way) => range(way.issueMeanMs)],
    ["issuing one frame's 16 moves, worst ms", (way) => range(way.issueWorstMs)],
    ["refreshes missed by the helper", (way) => way.missedRefreshes],
    ["first-to-last window landing in a frame, median ms", (way) => range(way.spreadMedianMs)],
    ["first-to-last window landing in a frame, worst ms", (way) => range(way.spreadWorstMs)],
    ["frames whose 16 windows landed in one refresh", (way) => `${way.framesLandedInOneRefresh} (${percent(way.framesLandedInOneRefreshShare)})`],
    ["latest landing after the vertical blank, worst ms", (way) => range(way.lastLandingAfterVblankWorstMs)],
    ["glides that landed on the exact pixel", (way) => way.landedOnThePixel],
    ["panes exposed on screen (of 16)", (way) => range(way.panesExposed)],
    ["launch length, seconds", (way) => range(way.launchSeconds)],
  ];
  const stalled = summary.stallMs > 0 ? `, main thread kept busy ${summary.stallMs} ms once per glide` : "";
  console.log(`kw-panes-mover bench: 16 windows, ${summary.runs} launches per way, two glides per launch${stalled}`);
  console.log(`Electron ${summary.machine?.electron}; display ${display?.width}x${display?.height} DIP at ${display?.scaleFactor}x, ${display?.refreshHz} Hz`);
  console.log("cells are median (min to max) over the glides\n");
  console.log(table([["", ...ways], ...rows.map(([label, cell]) => [label, ...measured.map((way) => String(cell(way)))])]));
  for (const way of ways) {
    for (const failure of summary.ways[way].failedLaunches) console.log(`\n${way}: FAILED launch: ${failure}`);
    if (summary.ways[way].helperExits.length > 0) console.log(`\n${way} helper exits: ${summary.ways[way].helperExits.join("; ")}`);
  }
  console.log(`\nborder: ${JSON.stringify(summary.border)}`);
  console.log(`geometry of pane 1: ${JSON.stringify(summary.geometry?.[0])}`);
  console.log(`worst gap between probe polls: ${Math.max(...measured.map((way) => way.probeWorstPollGapMs))} ms`);
  console.log(`processes left behind: ${summary.leftBehind.length === 0 ? "none" : summary.leftBehind.join(", ")}`);
}

function range(spread, format = (value) => String(value)) {
  if (!spread) return "-";
  return `${format(spread.median)} (${format(spread.min)} to ${format(spread.max)})`;
}

function percent(share) {
  return `${round(share * 100, 1)}%`;
}

function table(rows) {
  const widths = rows[0].map((_, column) => Math.max(...rows.map((row) => row[column].length)));
  return rows.map((row) => row.map((cell, column) => cell.padEnd(widths[column])).join("  ").trimEnd()).join("\n");
}

function fail(message) {
  console.error(message);
  process.exitCode = 1;
}
