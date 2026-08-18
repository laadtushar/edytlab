/**
 * Which pane the left-hand side is showing.
 *
 * In its own module because both `App` and `AppHeader` need it, and a
 * component importing a type from the file that renders it is a cycle
 * waiting to happen.
 */
export type LeftView = "timeline" | "graph";
