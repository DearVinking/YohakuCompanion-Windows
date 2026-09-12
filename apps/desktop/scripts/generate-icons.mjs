// 生成占位应用图标（32x32 PNG + ico 内嵌 PNG）。正式图标替换 icons/ 下文件即可。
import zlib from 'node:zlib';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const outDir = path.join(path.dirname(fileURLToPath(import.meta.url)), '..', 'src-tauri', 'icons');
fs.mkdirSync(outDir, { recursive: true });

const W = 32;
const H = 32;

// 简单的「Y」字形配色：深蓝底 + 白色 Y
const pixels = Buffer.alloc(W * H * 4);
for (let y = 0; y < H; y++) {
  for (let x = 0; x < W; x++) {
    const i = (y * W + x) * 4;
    const nx = x - 15.5;
    const ny = y - 13;
    // Y 字形：两条斜臂 + 竖干
    const onLeftArm = Math.abs(nx + 7 - ny * 0.75) < 2.6 && ny < 0 && ny > -10;
    const onRightArm = Math.abs(nx - 7 + ny * 0.75) < 2.6 && ny < 0 && ny > -10;
    const onStem = Math.abs(nx) < 2.4 && ny >= 0 && y < 28;
    const on = onLeftArm || onRightArm || onStem;
    const rounded = Math.hypot(nx, y - 15.5) < 15.4;
    if (rounded && on) {
      pixels[i] = 255; pixels[i + 1] = 255; pixels[i + 2] = 255; pixels[i + 3] = 255;
    } else if (rounded) {
      pixels[i] = 30; pixels[i + 1] = 58; pixels[i + 2] = 138; pixels[i + 3] = 255;
    } else {
      pixels[i] = 0; pixels[i + 1] = 0; pixels[i + 2] = 0; pixels[i + 3] = 0;
    }
  }
}

// PNG 编码（无过滤逐行）
function crc32(buf) {
  let table = crc32.table;
  if (!table) {
    table = crc32.table = new Int32Array(256);
    for (let n = 0; n < 256; n++) {
      let c = n;
      for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
      table[n] = c;
    }
  }
  let crc = -1;
  for (const b of buf) crc = (crc >>> 8) ^ table[(crc ^ b) & 0xff];
  return (crc ^ -1) >>> 0;
}
function chunk(type, data) {
  const len = Buffer.alloc(4);
  len.writeUInt32BE(data.length);
  const body = Buffer.concat([Buffer.from(type), data]);
  const crc = Buffer.alloc(4);
  crc.writeUInt32BE(crc32(body));
  return Buffer.concat([len, body, crc]);
}
const ihdr = Buffer.alloc(13);
ihdr.writeUInt32BE(W, 0);
ihdr.writeUInt32BE(H, 4);
ihdr[8] = 8; ihdr[9] = 6; // 8bit RGBA
const raw = Buffer.concat(
  Array.from({ length: H }, (_, y) =>
    Buffer.concat([Buffer.from([0]), pixels.subarray(y * W * 4, (y + 1) * W * 4)]),
  ),
);
const png = Buffer.concat([
  Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
  chunk('IHDR', ihdr),
  chunk('IDAT', zlib.deflateSync(raw)),
  chunk('IEND', Buffer.alloc(0)),
]);
fs.writeFileSync(path.join(outDir, 'icon.png'), png);

// ICO（BMP 条目，rc.exe 兼容）：40 字节 BITMAPINFOHEADER + XOR(BGRA, bottom-up) + AND mask
const xorStride = W * 4;
const andStride = Math.ceil(W / 32) * 4;
const xor = Buffer.alloc(xorStride * H);
const and = Buffer.alloc(andStride * H);
for (let y = 0; y < H; y++) {
  const srcRow = H - 1 - y; // bottom-up
  pixels.copy(xor, y * xorStride, srcRow * W * 4, srcRow * W * 4 + W * 4);
  // BGRA：PNG 像素是 RGBA
  for (let x = 0; x < W; x++) {
    const d = y * xorStride + x * 4;
    const sIdx = srcRow * W * 4 + x * 4;
    xor[d] = pixels[sIdx + 2];
    xor[d + 1] = pixels[sIdx + 1];
    xor[d + 2] = pixels[sIdx];
    xor[d + 3] = pixels[sIdx + 3];
  }
}
const bmpHeader = Buffer.alloc(40);
bmpHeader.writeUInt32LE(40, 0);
bmpHeader.writeInt32LE(W, 4);
bmpHeader.writeInt32LE(H * 2, 8); // XOR + AND
bmpHeader.writeUInt16LE(1, 12);
bmpHeader.writeUInt16LE(32, 14);
bmpHeader.writeUInt32LE(0, 16);
bmpHeader.writeUInt32LE(xor.length + and.length, 20);
const image = Buffer.concat([bmpHeader, xor, and]);

const count = 1;
const header = Buffer.alloc(6);
header.writeUInt16LE(0, 0); // idReserved
header.writeUInt16LE(1, 2); // idType: 1 = icon
header.writeUInt16LE(count, 4);
const entry = Buffer.alloc(16);
entry[0] = W; entry[1] = H; entry[2] = 0; entry[3] = 0;
entry.writeUInt16LE(1, 4); entry.writeUInt16LE(32, 6);
entry.writeUInt32LE(image.length, 8);
entry.writeUInt32LE(22, 12);
fs.writeFileSync(path.join(outDir, 'icon.ico'), Buffer.concat([header, entry, image]));
console.log('icons generated:', outDir);
