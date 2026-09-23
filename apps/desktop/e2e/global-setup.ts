/**
 * Runs once before any test: writes the audio fixtures the tests load,
 * so each run decodes files generated from the same arithmetic.
 */

import { writeFixtures } from "./audio-fixtures";

export default function globalSetup(): void {
  writeFixtures();
}
