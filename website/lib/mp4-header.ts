/**
 * What an MP4 says about itself, read from its header boxes with no tool
 * installed, so a test can hold a list's numbers to the file they
 * describe: the running time (`moov` > `mvhd`) and the picture's size
 * (`moov` > `trak` > `tkhd`).
 *
 * Used by `demos.test.ts` and `videos.test.ts`. It reads a Buffer and
 * touches nothing else, so it is a helper for tests and not part of the
 * site.
 */

/** A box found by `find`: where its payload starts, and where the box ends. */
type Span = [payloadStart: number, end: number];

/** The first box of `type` between `start` and `end` in `file`. */
function find(file: Buffer, start: number, end: number, type: string): Span | null {
  return all(file, start, end, type)[0] ?? null;
}

/** Every box of `type` between `start` and `end` in `file`, in order. */
function all(file: Buffer, start: number, end: number, type: string): Span[] {
  const out: Span[] = [];
  let at = start;
  while (at + 8 <= end) {
    let size = file.readUInt32BE(at);
    let header = 8;
    if (size === 1) {
      size = Number(file.readBigUInt64BE(at + 8));
      header = 16;
    } else if (size === 0) {
      size = end - at;
    }
    if (size < header) break;
    if (file.toString("latin1", at + 4, at + 8) === type) out.push([at + header, at + size]);
    at += size;
  }
  return out;
}

/** The `moov` box's span, or an error naming what was not an MP4. */
function moovOf(file: Buffer): Span {
  const moov = find(file, 0, file.length, "moov");
  if (!moov) throw new Error("no moov box: not an MP4 this reader can read");
  return moov;
}

/** The running time in seconds, from `mvhd`: a duration in units of its timescale. */
export function mp4Seconds(file: Buffer): number {
  const moov = moovOf(file);
  const mvhd = find(file, moov[0], moov[1], "mvhd");
  if (!mvhd) throw new Error("no mvhd box: not an MP4 this reader can read");
  const at = mvhd[0];
  const version = file.readUInt8(at);
  const timescale = file.readUInt32BE(at + (version === 1 ? 20 : 12));
  const duration = version === 1 ? Number(file.readBigUInt64BE(at + 24)) : file.readUInt32BE(at + 16);
  return duration / timescale;
}

/**
 * The size in pixels of the picture, from the first track whose `tkhd`
 * gives a width and a height (an audio track gives zero for both).
 */
export function mp4Size(file: Buffer): { width: number; height: number } {
  const moov = moovOf(file);
  for (const [start, end] of all(file, moov[0], moov[1], "trak")) {
    const tkhd = find(file, start, end, "tkhd");
    if (!tkhd) continue;
    const at = tkhd[0];
    // Width and height are 16.16 fixed-point numbers at the end of the box.
    const sizeAt = at + (file.readUInt8(at) === 1 ? 88 : 76);
    const width = file.readUInt32BE(sizeAt) / 65536;
    const height = file.readUInt32BE(sizeAt + 4) / 65536;
    if (width > 0 && height > 0) return { width, height };
  }
  throw new Error("no video track with a size: not an MP4 this reader can read");
}
