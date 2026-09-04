// Source of truth: assets/icons/logo-ga.svg.
// Requires Node.js and sharp (normal module resolution or NODE_PATH).
// Run from any directory: node scripts/generate-brand-icons.cjs
const fs = require("node:fs");
const path = require("node:path");
const sharp = require("sharp");

async function main() {
  const root = path.resolve(__dirname, "..");
  const iconDir = path.join(root, "assets/icons");
  const websiteDir = path.join(root, "website/public/assets");
  const svg = fs.readFileSync(path.join(iconDir, "logo-ga.svg"));
  const png = await sharp(svg, { density: 288 }).resize(256, 256).png().toBuffer();
  // PNG-compressed 256px ICO, matching the existing Windows packaging contract.
  const header = Buffer.alloc(22);
  header.writeUInt16LE(1, 2);
  header.writeUInt16LE(1, 4);
  header.writeUInt16LE(1, 10);
  header.writeUInt16LE(32, 12);
  header.writeUInt32LE(png.length, 14);
  header.writeUInt32LE(22, 18);
  fs.writeFileSync(path.join(iconDir, "logo-ga.png"), png);
  const ico = Buffer.concat([header, png]);
  fs.writeFileSync(path.join(iconDir, "git-agent.ico"), ico);
  fs.writeFileSync(path.join(websiteDir, "..", "favicon.ico"), ico);
  fs.writeFileSync(path.join(websiteDir, "logo-ga.svg"), svg);
  fs.writeFileSync(path.join(websiteDir, "logo-ga.png"), png);
  console.log("Generated app PNG/ICO and website SVG/PNG from logo-ga.svg.");
}

main().catch(error => { console.error(error); process.exitCode = 1; });
