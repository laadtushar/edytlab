import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const HERE = dirname(fileURLToPath(import.meta.url));

/** The record folder. */
export const RECORD_DIR = resolve(HERE, "..");
/** Where the finished footage goes. */
export const FOOTAGE_DIR = resolve(RECORD_DIR, "../footage");
