/**
 * Text assistance off, for a field whose text is an identifier, a path, a
 * command, a key or a pattern (#735).
 *
 * macOS's autocorrect offers the capitalised form of `test-delete` and
 * accepts it when the field loses focus, so the skill was saved as
 * `Test-delete.md`. Spread this onto every such `<input>` / `<textarea>`:
 * `<input {...IDENTIFIER_INPUT} />`. Free prose (a description, a skill or
 * system-prompt body) keeps the defaults. `identifierInputs.test.ts` fails
 * when a field in the editors and Settings has neither.
 */
export const IDENTIFIER_INPUT = {
  autoCapitalize: "off",
  autoCorrect: "off",
  spellCheck: false,
} as const;
