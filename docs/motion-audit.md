# Motion audit — the desktop app

Phase one of #211. The deliverable is the verdict on every component,
including the ones that should stay still, because "add some animation"
without that list is how a tool ends up slower for the people who use it
all day.

## The bar

Motion in a tool earns its place by doing one of three jobs:

1. **Explaining a change** — where did that come from, what is different now.
2. **Confirming an action landed** — the difference between "my click
   registered" and "is it broken?".
3. **Covering a wait** — occupying time something genuinely takes.

An audio editor is open for hours. Anything that delights once and costs
150 ms on the four-hundredth repetition is a net loss. If a reviewer
cannot name which of the three jobs a piece of motion does, it should not
ship.

## What was actually there

Measured, not assumed:

| | count |
|---|---|
| Components in `apps/desktop/src/components` | 30 |
| Components with any motion at all | 13 |
| Distinct motion utilities in use | 5 |
| Occurrences of `prefers-reduced-motion` | **0** |
| Named durations or easings | **0** |

The five utilities are `transition` (bare, 12 uses), `transition-colors`,
`transition-opacity`, `animate-pulse` (5) and `animate-spin`. Nearly all
of it is bare `transition`, which is Tailwind's default — 150 ms,
`cubic-bezier(0.4, 0, 0.2, 1)`, applied to every animatable property. So
the app does have a de-facto duration; nobody chose it, and nothing
records that it was chosen.

`styles.css` opens by describing itself as the "single source for color,
typography, motion". It defines twenty-odd colour tokens, three font
stacks, and **no motion tokens whatsoever**. One keyframe pair
(`app-fade-in` at 320 ms, `wave-pulse`) sits at the bottom of the file
outside the token block.

The zero for `prefers-reduced-motion` is the part that isn't a style
question. Vestibular-disorder triggers are a real accessibility failure,
and the OS-level setting is currently ignored everywhere in the app.

## The library question

The ticket asks for a number rather than a preference. The number that
settles it is **three**, and it isn't bundle size.

Tauri does not ship a browser — it uses the host's webview. That is
**WKWebView on macOS, WebView2 (Chromium) on Windows, and WebKitGTK on
Linux**: three engines, three release cadences, three sets of
whatever-the-distro-shipped. Bundle size genuinely is close to free here,
because assets load from local disk; what is not free is verifying
behaviour three times.

That reframes the choice. GSAP's cost in this app isn't its ~23 KB
gzipped core, it's that it becomes a second motion vocabulary to keep in
step with the website's, for a set of effects that are mostly CSS
transitions wearing a JS API. The same reasoning rules out leaning on the
View Transitions API, which would give layout animation for 0 KB and is
exactly the kind of recent feature where WebKitGTK on an LTS distro is a
coin toss.

**Decision: no library.** CSS custom properties for the vocabulary, plain
CSS transitions and keyframes for the motion. Every engine has supported
those identically for a decade. The one case that genuinely wants
scripting — FLIP for clips moving after an edit — is a small, contained
helper, and is deferred to its own change rather than used to justify a
dependency for the whole app.

If a future need arises that CSS genuinely cannot express, this decision
should be revisited with that specific need named. It should not be
revisited because GSAP is nicer to write.

## The vocabulary

Three durations and two easings, named in `styles.css`:

| Token | Value | For |
|---|---|---|
| `--dur-1` | 120 ms | State echo — hover, press, focus. Below ~100 ms reads as instant; above ~200 ms reads as lag on a control you are holding. |
| `--dur-2` | 200 ms | Something entering or leaving — overlays, banners, the progress strip. |
| `--dur-3` | 320 ms | Something being explained — a change that has to be followed. Matches the existing `app-fade-in`, which was already the app's de-facto "explain" duration. |
| `--ease-out` | `cubic-bezier(0.2, 0.65, 0.3, 1)` | Anything arriving or responding. Decelerating: fast to start so it feels immediate, settling so it doesn't stop dead. |
| `--ease-in-out` | `cubic-bezier(0.4, 0, 0.2, 1)` | Anything moving between two on-screen positions. This is Tailwind's default, kept deliberately so the migration changes no timing that was already right. |

Three, not two, because "responds to my finger", "arrives on screen" and
"explains what happened" are genuinely three different jobs. A fourth
would be someone wanting a specific number, which is what this table
exists to prevent.

## Reduced motion

Honoured globally rather than per-component, because a per-component
opt-in is a list that is one component out of date the moment anybody
adds a component:

```css
@media (prefers-reduced-motion: reduce) {
  *, *::before, *::after {
    animation-duration: 0.01ms !important;
    animation-iteration-count: 1 !important;
    transition-duration: 0.01ms !important;
    scroll-behavior: auto !important;
  }
}
```

`0.01ms` rather than `0` so `transitionend` / `animationend` still fire —
anything sequencing off those keeps working instead of hanging. The
matching `useReducedMotion()` hook covers the JS-driven cases, and
subscribes to the media query so a mid-session change of the OS setting
is picked up without a restart.

## The component table

Verdict on all 30, plus `App.tsx`.

### Changed here

| Component | The change today | Job | What it does now |
|---|---|---|---|
| `CommandPalette` | `return null` when closed — appears and vanishes between frames | 2 | Scale-and-fade in over `--dur-2`; leaves the same way instead of blinking out |
| `ShortcutsOverlay` | Same | 2 | Same |
| `TemplatePickerModal` | Same | 2 | Same |
| `ToolProgressBar` | Same, and it is *specifically* the thing that covers a wait | 3 | Slides down on arrival, collapses on leave. The fill already eased at 200 ms; that now names the token instead of hard-coding it |
| 13 components with bare `transition` | Tailwind's unchosen default | 1 | Same timing, now a named token — `--dur-1` is 120 ms rather than 150 ms, the one deliberate change |
| Everything | `prefers-reduced-motion` ignored | — | Honoured globally |

### Already correct — deliberately unchanged

| Component | Why |
|---|---|
| `ErrorBanner` | Already enters on `app-fade-in`. The ticket lists it as appearing instantly; that is not what the code does. Retimed to the vocabulary, no behavioural change |
| `MessageBubble` | Already fades in per message. Correct as-is |
| `EmptyState` | Already fades in |
| `LabelLane` | Drag is tracked on `window` and follows the pointer live. This is the app's best micro-interaction and the reference for the others; the commit snap is left for the clip-motion change, where it belongs with the rest of the timeline |
| `ThinkingIndicator` | `animate-pulse` covering a wait. Doing its job |

### Deliberately not animating

The reason each one is a *no*, since that is the half of the audit that
prevents the next person from "fixing" it:

| Component | Why not |
|---|---|
| `Canvas`, `Ruler`, playhead | On the playback path. The playhead already moves — animating its motion would animate an animation, and any layout-driven transition here shows up as jitter at high zoom, on the one surface where jitter reads as an audio problem |
| `SpectrumChart` | Redraws from analysis data. A transition between two spectra is a picture of neither |
| `MarkerLayer` | Positions are data, not state. A marker that eases to a new x is lying about when it moved |
| `TrackMenu`, `CapabilitiesMenu` | Context menus. Appearing under the cursor *now* is the entire contract; 200 ms of scale is 200 ms of not being able to click |
| `Settings`, `McpServersEditor`, `SkillsEditor`, `AgentProfilesEditor`, `MemoryEditor` | Forms. Their hover and focus states are covered by the vocabulary migration; beyond that there is no change to explain |
| `RecentProjects`, `TranscriptPane`, `ToolBadge`, `Chat`, `AppHeader` | Content surfaces whose changes are self-evident from the content itself |
| Waveform redraw | Same reason as the spectrum, and it is on the render path |

### Deferred, with a reason

| Component | Job | Why not here |
|---|---|---|
| `Timeline` / `ClipStrip` | 1 — the biggest one in the app | A cut or a move is *the* change that needs explaining, and doing it properly means FLIP against real layout. It is a different kind of change from a vocabulary migration and wants its own review |
| `GraphView` | 1 | Branching is the product's core idea and is currently silent. Node entry and head movement are worth showing; it is also 478 lines of `@xyflow/react` with its own layout pass, so it is not a token migration |
| `AutomationLane` | 1 | A curve drawing itself would explain what `duck_under_speech` did. Wants the timeline work first — the two share a coordinate space |
| `ABCompareBar` | 1 | A crossfade would make the comparison legible as a comparison. Small, but it belongs with the timeline change |

## Phase 2 — micro-interactions

Distinct from transitions: the feedback that makes direct manipulation
feel direct. Two gaps, both found by looking rather than guessing.

**Focus was invisible.** `--ring-focus` had been sitting in the token
block since the theme landed, referenced by nothing, and the app
contained **zero** occurrences of `focus-visible`. A keyboard user got
whatever the webview's default outline is against a near-black
background — faint on WebKit, effectively absent in the darker panels.
This is the cheapest and most valuable micro-interaction in the app,
because it is the difference between tabbing being usable and being a
guess. Hung off `:focus-visible` rather than `:focus`, so a mouse click
leaves nothing behind — using `:focus` is what leads people to delete
focus styling altogether.

**The faders did not acknowledge the grab.** The *response* half was
already right: `onChange` updates the readout every frame while the
value is held, and only `onPointerUp` writes a session node, so one
drag is one undo step rather than one per pixel. What was missing was
any sign that the thumb was under the pointer or being held, so the
control read as a picture of a fader until the number started moving.
The thumb now grows on hover and again on press — 1.15 and 1.3, small
on purpose, because a mixing control dragged hundreds of times must not
become a thing that moves while you are aiming at it.

**The clip chip said `grab` and kept saying it while being dragged**, so
the one moment the pointer was actually holding something looked
identical to hovering over it.

Measured against the ticket's own reference point: `LabelLane`'s drag
was already the app's best micro-interaction, and the faders fell short
of it not in responsiveness but in ever admitting they had been
touched.

## What this does not do

No decorative motion was added. Nothing was animated because it could be.
The four deferred items are the ones with real value left in them, and
they are deferred because they are a different piece of work, not because
they are optional.
