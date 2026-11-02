// Renders ascii-magic-art pieces (MIT, https://www.ascii-magic.com/ascii) into the
// frames the idle screen plays. Run it after changing the list below:
//   npm pack ascii-magic-art@1.1.0 --ignore-scripts && tar -xzf ascii-magic-art-1.1.0.tgz
//   node scripts/screensaver.mjs package projects/tui/assets/screensaver.bin
// Each piece becomes a u32 LE length and a raw-deflated text block: a header line
// "name\tcols\trows\tfps", a palette line "key=#rrggbb ...", then every frame as `rows`
// lines of characters followed by `rows` lines of color keys (space = default ink).
import { deflateRawSync } from "node:zlib";
import { writeFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { pathToFileURL } from "node:url";

// Calm, slow pieces that read well in a terminal. The two photo scenes keep the
// Unsplash license and stay out.
const PIECES = [
  "moonlit-sea", "northern-lights", "campfire-under-stars", "snowy-cabin",
  "rainy-window", "aurora-cabin-snowfall", "lighthouse-storm", "fuji-dawn-lake",
  "neon-city-rain", "underwater-reef", "desert-camp-milky-way", "cherry-blossom-river",
];
const SECONDS = 8;
const MAX_FPS = 12;

const [pkg, out] = process.argv.slice(2);
if (!pkg || !out) throw new Error("usage: screensaver.mjs <package dir> <out file>");

const parts = [];
for (const slug of PIECES) {
  const url = pathToFileURL(join(resolve(pkg), "pieces", `${slug}.js`)).href;
  const { meta, default: create } = await import(url);
  const draw = create();
  const fps = Math.min(meta.fps ?? MAX_FPS, MAX_FPS);
  const shape = (block) => {
    const rows = block.split("\n").slice(0, meta.rows);
    while (rows.length < meta.rows) rows.push("");
    return rows;
  };
  const palette = Object.entries(meta.palette ?? {}).map(([key, hex]) => `${key}=${hex}`);
  const lines = [[meta.name, meta.cols, meta.rows, fps].join("\t"), palette.join(" ")];
  for (let index = 0; index < SECONDS * fps; index++) {
    const frame = draw(index / fps);
    const text = typeof frame === "string" ? frame : frame.text;
    const color = typeof frame === "string" ? "" : frame.color ?? "";
    lines.push(...shape(text), ...shape(color));
  }
  const packed = deflateRawSync(Buffer.from(lines.join("\n") + "\n"), { level: 9 });
  const length = Buffer.alloc(4);
  length.writeUInt32LE(packed.length);
  parts.push(length, packed);
  console.log(`${slug}: ${meta.cols}x${meta.rows}, ${fps} fps, ${Math.round(packed.length / 1024)} KiB`);
}
writeFileSync(out, Buffer.concat(parts));
