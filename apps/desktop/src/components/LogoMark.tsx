/**
 * The edytlab mark: three level bars and two rising strokes.
 *
 * The same artwork as the site's header logo
 * (`website/components/motion/logo.tsx`) and `website/public/logo.svg`,
 * and as the app icon under `src-tauri/icons/`. It is drawn inline, in
 * the shapes' own coordinates (the site's group transform folded into
 * the `viewBox`), so a later pass can move the bars and strokes one at a
 * time; the `data-logo-bar` / `data-logo-line` hooks and
 * `pathLength="1"` are for that.
 *
 * Decorative. It is `aria-hidden` and not focusable, so putting it next
 * to a name never changes the name: the header's wordmark and the empty
 * state's `h1` are still just "edytlab".
 */

interface LogoMarkProps {
  /** Rendered width and height in px. */
  size?: number;
  className?: string;
}

export function LogoMark({ size = 20, className }: LogoMarkProps) {
  return (
    <svg
      viewBox="3.854 3.354 57.29 57.29"
      width={size}
      height={size}
      className={className}
      aria-hidden="true"
      focusable="false"
    >
      <g fill="var(--logo-violet)">
        <rect data-logo-bar x="5" y="26" width="6" height="12" rx="3" />
        <rect data-logo-bar x="14" y="20" width="6" height="24" rx="3" />
        <rect data-logo-bar x="23" y="25" width="6" height="14" rx="3" />
      </g>
      <path
        data-logo-line
        pathLength={1}
        d="M34 26 L41 18 L48 23 L57 12"
        fill="none"
        stroke="var(--logo-violet)"
        strokeWidth="6"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
      <path
        data-logo-line
        pathLength={1}
        d="M34 38 L41 46 L48 41 L57 52"
        fill="none"
        stroke="var(--logo-indigo)"
        strokeWidth="6"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  );
}
