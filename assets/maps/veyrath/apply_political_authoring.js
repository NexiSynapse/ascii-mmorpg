/* Apply the reviewable political plan to an Azgaar geography checkpoint.
 *
 * This is intentionally a checkpoint-producing utility. It loads the raw
 * geography map, changes only political/canon records, captures official Full
 * JSON + SVG from the live Azgaar page, and leaves the source checkpoint intact.
 * It uses the standalone @playwright/mcp browser used for the authoring pass.
 */

const fs = require("node:fs");
const path = require("node:path");
const { spawn } = require("node:child_process");

const ROOT = "C:\\Games\\ASCII-Game\\assets\\maps\\veyrath";
const MCP_ROOT = "C:\\Users\\kladd\\.config\\opencode";
const SOURCE_MAP = path.join(MCP_ROOT, "veyrath-03-geography.map");
const PLAN_PATH = path.join(ROOT, "political-authoring-plan.json");
const TEMP_FULL = path.join(MCP_ROOT, "veyrath-04-political-full.json");
const TEMP_SVG = path.join(MCP_ROOT, "veyrath-04-political.svg");

const plan = JSON.parse(fs.readFileSync(PLAN_PATH, "utf8"));

const server = spawn(
  "npx",
  ["-y", "@playwright/mcp", "--browser", "chrome", "--no-sandbox", "--shared-browser-context"],
  { cwd: MCP_ROOT, stdio: ["pipe", "pipe", "pipe"] }
);

let buffer = "";
let nextId = 1;
const pending = new Map();

server.stdout.on("data", chunk => {
  buffer += chunk.toString();
  const lines = buffer.split(/\r?\n/);
  buffer = lines.pop();
  for (const line of lines) {
    if (!line.trim()) continue;
    let message;
    try {
      message = JSON.parse(line);
    } catch {
      continue;
    }
    if (!message.id || !pending.has(message.id)) continue;
    const request = pending.get(message.id);
    pending.delete(message.id);
    if (message.error) request.reject(new Error(JSON.stringify(message.error)));
    else request.resolve(message.result);
  }
});
server.stderr.on("data", chunk => process.stderr.write(chunk));

function request(method, params) {
  return new Promise((resolve, reject) => {
    const id = nextId++;
    pending.set(id, { resolve, reject });
    server.stdin.write(`${JSON.stringify({ jsonrpc: "2.0", id, method, params })}\n`);
  });
}

async function call(name, args = {}) {
  const result = await request("tools/call", { name, arguments: args });
  const text = (result?.content ?? [])
    .filter(part => part.type === "text")
    .map(part => part.text)
    .join("\n");
  if (result?.isError) throw new Error(text || `${name} failed`);
  return text;
}

async function find(text, pattern) {
  const result = await call("browser_find", { text });
  const ref = result.match(pattern)?.[1];
  if (!ref) throw new Error(`Could not find ${text}\n${result}`);
  return ref;
}

function decodeEvaluationFile(filePath) {
  const raw = fs.readFileSync(filePath, "utf8");
  try {
    return JSON.parse(raw);
  } catch {
    if (raw.trimStart().startsWith("<svg")) return raw;
    const result = raw.match(/### Result\n([\s\S]*?)(?:\n### Ran Playwright code|$)/)?.[1]?.trim();
    if (result) return JSON.parse(result.replace(/^```(?:json)?\s*|\s*```$/g, ""));
    throw new Error(`Cannot decode evaluation result ${filePath}`);
  }
}

function applyPlan(planData) {
  const cells = pack.cells;
  const stateRows = planData.factions;
  const stateByFaction = Object.fromEntries(stateRows.map(row => [row.faction_id, row]));
  const factionByCulture = Object.fromEntries(stateRows.map(row => [row.generated_culture_id, row]));
  const religionByRegion = {};
  for (const religion of planData.religions) {
    for (const region of religion.regions) religionByRegion[region] = religion.generated_id;
  }
  const factionColors = Object.fromEntries(stateRows.map(row => [row.region_id, row.color]));
  const selectedStateIds = new Set(stateRows.map(row => row.generated_state_id));
  const selectedCultureIds = new Set(planData.cultures.map(row => row.generated_id));
  const selectedReligionIds = new Set(planData.religions.map(row => row.generated_id));
  const seeds = stateRows.map(row => ({
    faction: row.faction_id,
    region: row.region_id,
    point: [row.territorial_seed[0] * options.map.graph.width / 100, row.territorial_seed[1] * options.map.graph.height / 100]
  }));
  const neutralAnchors = planData.neutral_frontier.anchors.map(anchor => [
    anchor[0] * options.map.graph.width / 100,
    anchor[1] * options.map.graph.height / 100
  ]);
  const distance = (a, b) => Math.hypot(a[0] - b[0], a[1] - b[1]);

  // Apply the six canonical records before assigning cells. This keeps the
  // generated IDs stable while eliminating generated political identities.
  for (const row of planData.cultures) {
    Cultures.rename(row.generated_id, row.name);
    Cultures.recolor(row.generated_id, factionColors[factionByCulture[row.generated_id].region_id]);
    pack.cultures[row.generated_id].note = row.identity;
  }
  for (const row of planData.religions) {
    Religions.rename(row.generated_id, row.name);
    Religions.recolor(row.generated_id, factionColors[row.regions[0]] ?? "#817468");
    pack.religions[row.generated_id].note = row.identity;
  }
  for (const row of stateRows) {
    States.rename(row.generated_state_id, row.name);
    States.recolor(row.generated_state_id, row.color);
    States.setCulture(row.generated_state_id, row.generated_culture_id);
    const state = pack.states[row.generated_state_id];
    state.fullName = row.name;
    state.formName = "";
    state.form = "";
    state.provinces = [];
  }

  // Reset old province ownership and apply the explicit nearest-seed policy.
  cells.province.fill(0);
  for (const province of pack.provinces) if (province?.i) province.removed = true;
  for (const cellId of cells.i) {
    if (cells.h[cellId] < 20 || !pack.features[cells.f[cellId]]?.land) {
      cells.state[cellId] = 0;
      cells.culture[cellId] = 0;
      cells.religion[cellId] = 0;
      continue;
    }
    const point = cells.p[cellId];
    const nearest = seeds.reduce((best, seed) => {
      const currentDistance = distance(point, seed.point);
      return currentDistance < best.distance ? { seed, distance: currentDistance } : best;
    }, { seed: seeds[0], distance: Number.POSITIVE_INFINITY });
    const neutralDistance = Math.min(...neutralAnchors.map(anchor => distance(point, anchor)));
    const unclaimed = neutralDistance <= Math.min(48, nearest.distance * 0.42);
    if (unclaimed) {
      cells.state[cellId] = 0;
      cells.culture[cellId] = 0;
      cells.religion[cellId] = 0;
      continue;
    }
    const faction = stateByFaction[nearest.seed.faction];
    cells.state[cellId] = faction.generated_state_id;
    cells.culture[cellId] = faction.generated_culture_id;
    cells.religion[cellId] = religionByRegion[faction.region_id] ?? 0;
  }

  // Remove generated records that are no longer part of Veyrath canon.
  for (const culture of pack.cultures) {
    if (culture?.i && !selectedCultureIds.has(culture.i) && !culture.removed) Cultures.remove(culture.i);
  }
  for (const religion of pack.religions) {
    if (religion?.i && !selectedReligionIds.has(religion.i) && !religion.removed) Religions.remove(religion.i);
  }
  for (const state of pack.states) {
    if (state?.i && !selectedStateIds.has(state.i) && !state.removed) States.remove(state.i);
  }

  // Existing burgs are renamed in place; flagged targets are created at the
  // plan's recommended viable cell. The raw geography checkpoint remains the
  // rollback source if a visual review rejects a site.
  const settlementResults = [];
  const factionForSettlement = settlement => stateRows.find(row => row.faction_id === settlement.faction);
  const groups = {capital: "capital", city: "city", fortified_town: "fort", village: "village", hamlet: "hamlet"};
  for (const settlement of planData.settlements) {
    let burgId = settlement.existing_burg_id;
    let created = false;
    try {
      if (!burgId || !pack.burgs[burgId] || pack.burgs[burgId].removed) {
        burgId = Burgs.add(settlement.target_xy[0], settlement.target_xy[1]);
        created = true;
      }
      const row = factionForSettlement(settlement);
      const burg = pack.burgs[burgId];
      burg.state = row.generated_state_id;
      burg.culture = row.generated_culture_id;
      Burgs.rename(burgId, settlement.name);
      Burgs.setCulture(burgId, row.generated_culture_id);
      Burgs.setGroup(burgId, groups[settlement.kind] ?? "village");
      if (settlement.population_target) Burgs.setPopulation(burgId, settlement.population_target);
      if (settlement.kind === "capital" || settlement.kind === "fortified_town") {
        Burgs.setBuilding(burgId, "citadel", true);
        Burgs.setBuilding(burgId, "walls", true);
      }
      if (settlement.kind === "capital" || settlement.kind === "city") Burgs.setBuilding(burgId, "plaza", true);
      if (["blackwake", "saltgrave", "brinewatch", "gullscar", "crowhaven"].includes(settlement.settlement_id)) {
        try {
          Burgs.setPort(burgId, true);
        } catch (error) {
          burg.note = `Port review required: ${error.message}`;
        }
      }
      settlementResults.push({settlement_id: settlement.settlement_id, burg_id: burgId, created, cell: burg.cell, port: burg.port ?? 0});
    } catch (error) {
      settlementResults.push({settlement_id: settlement.settlement_id, burg_id: burgId ?? null, created, error: error.message});
    }
  }

  // Make the six named capitals authoritative after every settlement has been
  // attached to its desired state.
  for (const faction of stateRows) {
    const capital = settlementResults.find(result => result.settlement_id === faction.capital_settlement);
    if (!capital || capital.burg_id == null || !pack.burgs[capital.burg_id]) continue;
    const state = pack.states[faction.generated_state_id];
    for (const burg of pack.burgs) {
      if (burg?.i && !burg.removed && burg.state === state.i && burg.capital && burg.i !== capital.burg_id) {
        burg.capital = 0;
        Burgs.changeGroup(burg, "city");
      }
    }
    const burg = pack.burgs[capital.burg_id];
    burg.state = state.i;
    burg.capital = 1;
    burg.group = "capital";
    state.capital = burg.i;
    state.center = burg.cell;
  }

  for (const burg of pack.burgs) {
    if (!burg?.i || burg.removed) continue;
    burg.state = cells.state[burg.cell];
    burg.culture = cells.culture[burg.cell];
  }
  for (const faction of stateRows) {
    const capital = settlementResults.find(result => result.settlement_id === faction.capital_settlement);
    if (capital?.burg_id != null && pack.burgs[capital.burg_id]) {
      const burg = pack.burgs[capital.burg_id];
      burg.state = faction.generated_state_id;
      burg.culture = faction.generated_culture_id;
      burg.capital = 1;
      burg.group = "capital";
      pack.states[faction.generated_state_id].capital = burg.i;
      pack.states[faction.generated_state_id].center = burg.cell;
    }
  }

  States.getPoles();
  States.findNeighbors();
  States.collectStatistics();
  Layers.drawAll();

  const living = values => values.filter(value => value?.i && !value.removed);
  return {
    schema: "veyrath-political-application-audit-v1",
    source: "veyrath-03-geography.map",
    states: living(pack.states).map(state => ({i: state.i, name: state.name, color: state.color, cells: state.cells, capital: state.capital})),
    cultures: living(pack.cultures).map(culture => ({i: culture.i, name: culture.name, color: culture.color, cells: culture.cells})),
    religions: living(pack.religions).map(religion => ({i: religion.i, name: religion.name, color: religion.color})),
    settlements: settlementResults,
    warnings: settlementResults.filter(result => result.error || result.burg_id == null || (result.port === 0 && ["blackwake", "saltgrave", "brinewatch", "gullscar", "crowhaven"].includes(result.settlement_id)))
  };
}

async function waitForFile(filePath, timeoutMs = 15000) {
  const start = Date.now();
  while (!fs.existsSync(filePath)) {
    if (Date.now() - start > timeoutMs) throw new Error(`Timed out waiting for ${filePath}`);
    await new Promise(resolve => setTimeout(resolve, 100));
  }
}

async function main() {
  await new Promise(resolve => setTimeout(resolve, 1500));
  await request("initialize", {
    protocolVersion: "2025-06-18",
    capabilities: {},
    clientInfo: {name: "veyrath-political-authoring", version: "1.0"}
  });
  server.stdin.write(`${JSON.stringify({jsonrpc: "2.0", method: "notifications/initialized", params: {}})}\n`);
  await call("browser_navigate", {url: "https://azgaar.github.io/Fantasy-Map-Generator/"});
  await new Promise(resolve => setTimeout(resolve, 10000));
  await call("browser_click", {target: await find("►", /button "►" \[ref=([^\]\s]+)/)});
  await call("browser_click", {target: await find("Load", /button "Load" \[ref=([^\]\s]+)/)});
  await call("browser_click", {target: await find("machine", /button "machine" \[ref=([^\]\s]+)/)});
  await call("browser_file_upload", {paths: [SOURCE_MAP]});
  await new Promise(resolve => setTimeout(resolve, 15000));

  const applyCode = `(${applyPlan.toString()})(${JSON.stringify(plan)})`;
  const auditText = await call("browser_evaluate", {function: `() => ${applyCode}`});
  console.log("POLITICAL_AUDIT\n" + auditText);

  if (fs.existsSync(TEMP_FULL)) fs.unlinkSync(TEMP_FULL);
  if (fs.existsSync(TEMP_SVG)) fs.unlinkSync(TEMP_SVG);
  await call("browser_evaluate", {
      function: `() => {
        const cellData = {
          v: pack.cells.v, c: pack.cells.c, p: pack.cells.p,
          g: Array.from(pack.cells.g), h: Array.from(pack.cells.h), area: Array.from(pack.cells.area),
          f: Array.from(pack.cells.f), t: Array.from(pack.cells.t), haven: Array.from(pack.cells.haven),
          harbor: Array.from(pack.cells.harbor), fl: Array.from(pack.cells.fl), r: Array.from(pack.cells.r),
          conf: Array.from(pack.cells.conf), biome: Array.from(pack.cells.biome), s: Array.from(pack.cells.s),
          pop: Array.from(pack.cells.pop), culture: Array.from(pack.cells.culture), burg: Array.from(pack.cells.burg),
          routes: pack.cells.routes, state: Array.from(pack.cells.state), religion: Array.from(pack.cells.religion),
          province: Array.from(pack.cells.province), good: Array.from(pack.cells.good || []),
          market: Array.from(pack.cells.market || [])
        };
        const packCells = {
          cells: Array.from(pack.cells.i).map(i => ({i, v: cellData.v[i], c: cellData.c[i], p: cellData.p[i], g: cellData.g[i], h: cellData.h[i], area: cellData.area[i], f: cellData.f[i], t: cellData.t[i], haven: cellData.haven[i], harbor: cellData.harbor[i], fl: cellData.fl[i], r: cellData.r[i], conf: cellData.conf[i], biome: cellData.biome[i], s: cellData.s[i], pop: cellData.pop[i], culture: cellData.culture[i], burg: cellData.burg[i], routes: cellData.routes[i], state: cellData.state[i], religion: cellData.religion[i], province: cellData.province[i], good: cellData.good[i], market: cellData.market[i]})),
          vertices: Array.from(pack.vertices.p).map((_, i) => ({i, p: pack.vertices.p[i], v: pack.vertices.v[i], c: pack.vertices.c[i]})),
          features: pack.features, biomes: pack.biomes, cultures: pack.cultures, burgs: pack.burgs, states: pack.states,
          provinces: pack.provinces, religions: pack.religions, rivers: pack.rivers, goods: pack.goods, markers: pack.markers,
          markets: pack.markets, deals: pack.deals, routes: pack.routes, zones: pack.zones, measurers: pack.measurers, journeys: pack.journeys
        };
        const gridCells = {
          cells: Array.from(grid.cells.i).map(i => ({i, v: grid.cells.v[i], c: grid.cells.c[i], b: grid.cells.b[i], f: grid.cells.f[i], t: grid.cells.t[i], h: grid.cells.h[i], temp: grid.cells.temp[i], prec: grid.cells.prec[i]})),
          vertices: Array.from(grid.vertices.p).map((_, i) => ({i, p: grid.vertices.p[i], v: grid.vertices.v[i], c: grid.vertices.c[i]})),
          spacing: grid.spacing, cellsY: grid.cellsY, cellsX: grid.cellsX, points: grid.points, boundary: grid.boundary, seed: options.map.seed, features: pack.features
        };
        return JSON.stringify({info: {version: "1.154.2", description: "Azgaar's Fantasy Map Generator output: azgaar.github.io/Fantasy-map-generator", exportedAt: new Date().toISOString(), mapName: options.map.lore.name, width: options.map.graph.width, height: options.map.graph.height, seed: options.map.seed, mapId: Date.now()}, settings: {distanceUnit: options.map.units.distance.unit, distanceScale: options.map.units.distance.scale, areaUnit: options.map.units.area.unit, heightUnit: options.map.units.height.unit, heightExponent: options.map.units.height.exponent, temperatureScale: options.map.units.temperature.unit, populationRate: options.map.units.population.scale, urbanization: options.map.units.population.urbanization.rate, urbanDensity: options.map.units.population.urbanization.density, options, mapName: options.map.lore.name, stylePreset: options.map.style.preset}, mapCoordinates: options.map.geography.coordinates, pack: packCells, grid: gridCells, nameBases: Names.nameBases});
      }`,
      filename: "veyrath-04-political-full.json"
  });
  await call("browser_evaluate", {
    function: `() => { const clone = document.querySelector("#map").cloneNode(true); clone.setAttribute("width", String(options.map.graph.width)); clone.setAttribute("height", String(options.map.graph.height)); clone.querySelector("#viewbox")?.removeAttribute("transform"); return new XMLSerializer().serializeToString(clone); }`,
    filename: "veyrath-04-political.svg"
  });
  await waitForFile(TEMP_FULL);
  await waitForFile(TEMP_SVG);

  const full = decodeEvaluationFile(TEMP_FULL);
  const svg = decodeEvaluationFile(TEMP_SVG);
  const fullPath = path.join(ROOT, "veyrath-04-political-full.json");
  const svgPath = path.join(ROOT, "veyrath-04-political.svg");
  fs.writeFileSync(fullPath, `${typeof full === "string" ? full : JSON.stringify(full)}\n`, "utf8");
  fs.writeFileSync(svgPath, typeof svg === "string" ? svg : JSON.stringify(svg), "utf8");
  console.log(`wrote ${fullPath}`);
  console.log(`wrote ${svgPath}`);
}

main()
  .catch(error => {
    console.error(error.stack || error);
    process.exitCode = 1;
  })
  .finally(() => setTimeout(() => server.kill(), 1000));
