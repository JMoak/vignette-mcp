"use strict";

// Runs under Electron. Opens sixteen frameless owned windows, glides them out and back one way
// (KW_BENCH_MODE = js | helper-async | helper-defer) or paints their borders through the helper
// (KW_BENCH_MODE = border), and writes what it measured as JSON. KW_BENCH_STALL_MS keeps the
// main thread busy for that long once in each glide.

const { app, BrowserWindow, screen } = require("electron");
const { spawn } = require("node:child_process");
const fs = require("node:fs");
const path = require("node:path");
const readline = require("node:readline");
const { performance } = require("node:perf_hooks");
const { analyse } = require("./analyse.cjs");
const { framePacer, glideFrameMs, glider, timerLatenessOn } = require("./js-glider.cjs");

const root = path.resolve(__dirname, "..");
const temp = path.join(__dirname, ".tmp");
const mode = process.env.KW_BENCH_MODE ?? "js";
const outFile = process.env.KW_BENCH_OUT ?? path.join(temp, `${mode}.json`);
const durationMs = Number(process.env.KW_BENCH_DURATION_MS ?? 1500);
const stallMs = Number(process.env.KW_BENCH_STALL_MS ?? 0);
const helperExe = path.join(root, "target", "release", "kw-panes-mover.exe");
const probeExe = path.join(root, "target", "release", "examples", "bench_probe.exe");

const paneCount = 16;
const columns = 4;
const paneSize = { width: 280, height: 170 };
const gap = 16;
const margin = 40;
const travel = { x: 480, y: 120 };
const backdrop = "#1b1d22";
const borderColour = "ff00aa";
const watchMarginMs = 300;
const runLimitMs = 30_000;

const children = new Set();

app.setPath("userData", path.join(temp, "user-data"));
setTimeout(() => leave(3, { mode, error: "the run took too long" }), runLimitMs);
app.whenReady().then(run).then(
  (result) => leave(0, result),
  (error) => leave(1, { mode, error: String(error?.stack ?? error) }),
);

async function run() {
  const stage = await openStage();
  const atRest = await look(stage.handles);
  stage.homeCorners = atRest.map(cornerOf);
  const driver = mode === "js" ? javascriptDriver(stage) : await helperDriver(stage, issueOf(mode));
  const glides = [];
  for (const [direction, places] of mode === "border" ? [] : [["out", stage.away], ["back", stage.home]]) {
    glides.push({ direction, ...(await measuredGlide(stage, driver, places)) });
    await pause(150);
  }
  const border = mode === "border" ? await driver.checkBorder() : null;
  const helperExit = driver.close ? await driver.close() : null;
  return { mode, durationMs, stallMs, machine: machine(), panesExposedAtStart: exposedCount(atRest), geometry: geometryOf(stage, atRest), glides, border, helperExit };
}

function issueOf(mode) {
  return mode === "helper-async" ? ["--issue=async"] : mode === "helper-defer" ? ["--issue=defer"] : [];
}

// The options are copied from surfaceWindowOptions in packages/desktop/src/main/surfaces/window.ts.
function surfaceWindowOptions(spec) {
  return {
    parent: spec.parent,
    modal: false,
    frame: false,
    transparent: false,
    show: false,
    skipTaskbar: true,
    alwaysOnTop: false,
    minimizable: false,
    maximizable: false,
    fullscreenable: false,
    fullscreen: false,
    kiosk: false,
    resizable: true,
    minWidth: 200,
    minHeight: 120,
    backgroundColor: spec.backdrop,
    title: spec.title,
  };
}

async function openStage() {
  const area = screen.getPrimaryDisplay().workArea;
  const home = Array.from({ length: paneCount }, (_, index) => homeOf(area, index));
  const away = home.map((point) => ({ x: point.x + travel.x, y: point.y + travel.y }));
  assertFits(area, away[paneCount - 1]);
  const consoleWindow = new BrowserWindow({ show: false, x: area.x + margin, y: area.y + margin, width: 480, height: 320, title: "kw bench console" });
  consoleWindow.showInactive();
  const panes = await Promise.all(home.map((point, index) => openPane(consoleWindow, index, point)));
  for (const pane of panes) pane.showInactive();
  await pause(700);
  return { consoleWindow, panes, home, away, handles: panes.map(handleOf) };
}

function homeOf(area, index) {
  return {
    x: area.x + margin + (index % columns) * (paneSize.width + gap),
    y: area.y + margin + Math.floor(index / columns) * (paneSize.height + gap),
  };
}

function assertFits(area, lastAway) {
  const fits = lastAway.x + paneSize.width <= area.x + area.width && lastAway.y + paneSize.height <= area.y + area.height;
  if (!fits) throw new Error(`the primary display work area ${JSON.stringify(area)} is too small for the bench`);
}

async function openPane(parent, index, point) {
  const pane = new BrowserWindow(surfaceWindowOptions({ parent, backdrop, title: `pane ${index + 1}` }));
  pane.setBounds({ ...point, ...paneSize });
  const page = `<body style="margin:0;background:${backdrop};color:#99a;font:13px sans-serif"><p style="margin:12px">pane ${index + 1}</p></body>`;
  await pane.loadURL(`data:text/html;charset=utf-8,${encodeURIComponent(page)}`);
  return pane;
}

function handleOf(pane) {
  return pane.getNativeWindowHandle().readBigUInt64LE(0).toString(16);
}

function geometryOf(stage, looks) {
  return [0, paneCount - 1].map((index) => ({
    pane: index + 1,
    electronBounds: stage.panes[index].getBounds(),
    electronBoundsInPixels: screen.dipToScreenRect(null, stage.panes[index].getBounds()),
    windowRect: looks[index].rect,
    clientRect: looks[index].client,
    dwmVisibleFrame: looks[index].frame,
  }));
}

async function measuredGlide(stage, driver, places) {
  const watched = processesToWatch(driver);
  const probe = await startProbe(["watch", String(durationMs + watchMarginMs), watched.map((entry) => entry.pid).join(","), ...stage.handles]);
  await driver.prepare?.();
  probe.mark();
  const cpuBefore = process.cpuUsage();
  const startedAt = performance.now();
  if (stallMs > 0) setTimeout(() => keepMainThreadBusy(stallMs), durationMs / 5);
  await driver.glide(places);
  const wallMs = performance.now() - startedAt;
  const cpu = process.cpuUsage(cpuBefore);
  probe.mark();
  const { issue, missedRefreshes } = await driver.report();
  const observed = analyse(await probe.finished, durationMs);
  await pause(100);
  const landed = await look(stage.handles);
  return {
    wallMs: round(wallMs),
    cpuMs: cpuByRole(watched, observed.marked),
    electronMainByTickAccounting: { cpuMs: round((cpu.user + cpu.system) / 1000), shareOfOneCore: round((cpu.user + cpu.system) / 1000 / wallMs) },
    issue,
    missedRefreshes,
    observed,
    landedOnTarget: stage.panes.every((pane, index) => samePlace(pane.getPosition(), places[index])),
    landedOnThePixel: landed.every((seen, index) => samePoint(cornerOf(seen), expectedCorner(stage, index, places[index]))),
    panesExposed: exposedCount(landed),
  };
}

function keepMainThreadBusy(ms) {
  const until = performance.now() + ms;
  while (performance.now() < until);
}

function expectedCorner(stage, index, place) {
  const [from, to] = [pixelOf(stage.home[index]), pixelOf(place)];
  const home = stage.homeCorners[index];
  return { x: home.x + to.x - from.x, y: home.y + to.y - from.y };
}

function cornerOf(seen) {
  const [x, y] = seen.rect.split(",").map(Number);
  return { x, y };
}

function exposedCount(looks) {
  return looks.filter((seen) => seen.exposed === "1").length;
}

function pixelOf(point) {
  return screen.dipToScreenPoint(point);
}

function samePoint(a, b) {
  return a.x === b.x && a.y === b.y;
}

function processesToWatch(driver) {
  const electron = app.getAppMetrics().map((metric) => ({ role: roleOf(metric.type), pid: metric.pid }));
  return driver.pid ? [...electron, { role: "helper", pid: driver.pid }] : electron;
}

function roleOf(type) {
  return { Browser: "electronMain", Tab: "electronRenderers", GPU: "electronGpu" }[type] ?? "electronOther";
}

function cpuByRole(watched, marked) {
  if (!marked) return null;
  const cpuMs = { electronMain: 0, electronRenderers: 0, electronGpu: 0, electronOther: 0 };
  watched.forEach((entry, index) => {
    cpuMs[entry.role] = round((cpuMs[entry.role] ?? 0) + marked.cpuMs[index]);
  });
  const share = (role) => (role in cpuMs ? round(cpuMs[role] / marked.wallMs) : null);
  return { ...cpuMs, betweenMarksMs: marked.wallMs, electronMainShareOfOneCore: share("electronMain"), helperShareOfOneCore: share("helper") };
}

function javascriptDriver(stage) {
  let frames = [];
  let moves = 0;
  const now = () => performance.now();
  const pacer = framePacer(
    { now, defer: (wake) => setImmediate(wake), sleep: (wake, ms) => setTimeout(wake, ms) },
    { frameMs: glideFrameMs, timerLatenessMs: timerLatenessOn(process.platform) },
  );
  const timed = (tick) => () => {
    const [before, movesBefore] = [now(), moves];
    tick();
    if (moves > movesBefore) frames.push(now() - before);
  };
  const glides = glider({ now, everyFrame: (tick) => pacer(timed(tick)) });
  const bodyOf = (pane) => ({
    position: () => pointOf(pane.getPosition()),
    moveTo: (point) => {
      moves += 1;
      pane.setPosition(point.x, point.y);
    },
  });
  return {
    glide: (places) => {
      [frames, moves] = [[], 0];
      return glides.run(stage.panes.map((pane, key) => ({ key, body: bodyOf(pane), to: places[key] })), durationMs);
    },
    report: () => ({ issue: issueSummary(frames), missedRefreshes: null }),
  };
}

async function helperDriver(stage, helperArguments) {
  const helper = startHelper(helperArguments);
  const ready = await helper.next((line) => line.startsWith("ready "));
  if (ready !== `ready 1 ${process.pid}`) throw new Error(`unexpected greeting: ${ready}`);
  let glideId = 0;
  const stats = async () => {
    helper.send("stats");
    return fieldsOf(await helper.next((line) => line.startsWith("stats ")));
  };
  return {
    pid: helper.pid,
    prepare: stats,
    glide: (places) => {
      glideId += 1;
      const targets = places.map((place, index) => {
        const [at, to] = [pixelOf(pointOf(stage.panes[index].getPosition())), pixelOf(place)];
        return `${stage.handles[index]}@${at.x},${at.y}>${to.x},${to.y}`;
      });
      helper.send(`glide ${glideId} ${durationMs} ${targets.join(" ")}`);
      return helper.next((line) => line === `settled ${glideId}`);
    },
    report: async () => {
      const spent = await stats();
      return { issue: { frames: spent.frames, meanMs: spent.mean_us / 1000, worstMs: spent.worst_us / 1000 }, missedRefreshes: spent.missed };
    },
    checkBorder: () => checkBorder(stage, helper, stats),
    close: () => helper.close(),
  };
}

function startHelper(helperArguments) {
  const child = spawnChild(helperExe, helperArguments);
  const waiting = [];
  const unread = [];
  const refusals = [];
  readline.createInterface({ input: child.stdout }).on("line", (line) => {
    if (line.startsWith("refused ")) refusals.push(line);
    const waiter = waiting.findIndex((entry) => entry.wanted(line));
    if (waiter >= 0) waiting.splice(waiter, 1)[0].resolve(line);
    else unread.push(line);
  });
  const exited = new Promise((resolve) => child.on("exit", (code, signal) => resolve({ code, signal })));
  return {
    pid: child.pid,
    refusals,
    send: (line) => child.stdin.write(`${line}\n`),
    next: (wanted) => {
      const found = unread.findIndex(wanted);
      if (found >= 0) return Promise.resolve(unread.splice(found, 1)[0]);
      return new Promise((resolve) => waiting.push({ wanted, resolve }));
    },
    close: async () => {
      const closedAt = performance.now();
      child.stdin.end();
      const exit = await exited;
      return { ...exit, msAfterInputClosed: round(performance.now() - closedAt), refusals };
    },
  };
}

async function checkBorder(stage, helper, barrier) {
  const edgesNow = () => probed("border", stage.handles);
  const paint = async (colour) => {
    for (const handle of stage.handles) helper.send(`border ${handle} ${colour}`);
    await barrier();
    await pause(150);
    return edgesNow();
  };
  const refusedBefore = helper.refusals.length;
  const before = await edgesNow();
  const painted = await paint(borderColour);
  const cleared = await paint("clear");
  const edges = (looks) => looks.map((seen) => seen.edge.split(",")[0]);
  const exposed = before.map((seen, index) => seen.exposed === "1" && painted[index].exposed === "1" && cleared[index].exposed === "1");
  const took = (index) => edges(before)[index] !== borderColour && edges(painted)[index] === borderColour && edges(cleared)[index] === edges(before)[index];
  return {
    colour: borderColour,
    refused: helper.refusals.slice(refusedBefore),
    readBack: [...new Set(painted.map((seen) => `DwmGetWindowAttribute(DWMWA_BORDER_COLOR) -> ${seen.get}`))],
    windowsExposed: exposed.filter(Boolean).length,
    edgePixelTookColourThenReverted: exposed.filter((isExposed, index) => isExposed && took(index)).length,
    edgePixels: { before: edges(before), painted: edges(painted), cleared: edges(cleared) },
    edgeOfFirstPane: { before: before[0].edge, painted: painted[0].edge, cleared: cleared[0].edge },
  };
}

function look(handles) {
  return probed("look", handles);
}

async function probed(question, handles) {
  const text = await (await startProbe([question, ...handles])).finished;
  const lines = text.split(/\r?\n/).filter((line) => line.startsWith(`${question} `));
  return lines.map((line) => {
    const [, window, ...fields] = line.split(" ");
    return { window, ...Object.fromEntries(fields.map((field) => field.split("="))) };
  });
}

function startProbe(args) {
  const child = spawnChild(probeExe, args);
  let text = "";
  let announce;
  const watching = new Promise((resolve) => {
    announce = resolve;
  });
  child.stdout.setEncoding("utf8");
  child.stdout.on("data", (chunk) => {
    text += chunk;
    if (text.includes("\n")) announce();
  });
  const finished = new Promise((resolve) => child.on("close", () => resolve(text)));
  const started = args[0] === "watch" ? watching : Promise.resolve();
  return started.then(() => ({ finished, mark: () => child.stdin.write("mark\n") }));
}

function spawnChild(file, args) {
  const child = spawn(file, args, { stdio: ["pipe", "pipe", "ignore"], windowsHide: true });
  children.add(child);
  child.on("exit", () => children.delete(child));
  return child;
}

function issueSummary(frames) {
  const total = frames.reduce((sum, ms) => sum + ms, 0);
  return { frames: frames.length, meanMs: round(total / Math.max(1, frames.length)), worstMs: round(Math.max(0, ...frames)) };
}

function fieldsOf(line) {
  const fields = line.split(" ").slice(1).map((field) => field.split("="));
  return Object.fromEntries(fields.map(([name, value]) => [name, Number(value)]));
}

function machine() {
  const display = screen.getPrimaryDisplay();
  return {
    electron: process.versions.electron,
    chrome: process.versions.chrome,
    display: { ...display.size, scaleFactor: display.scaleFactor, refreshHz: display.displayFrequency },
  };
}

function pointOf([x, y]) {
  return { x, y };
}

function samePlace([x, y], place) {
  return x === place.x && y === place.y;
}

function pause(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

function round(value) {
  return Number(value.toFixed(3));
}

function leave(code, result) {
  for (const child of children) child.kill();
  fs.mkdirSync(path.dirname(outFile), { recursive: true });
  fs.writeFileSync(outFile, `${JSON.stringify(result, null, 2)}\n`);
  for (const window of BrowserWindow.getAllWindows()) window.destroy();
  app.exit(code);
}
