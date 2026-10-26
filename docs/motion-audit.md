---
title: "Motion audit"
description: "The bar a piece of motion has to clear in the desktop app, and a verdict on every component."
---

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
| `--dur-3` | 320 ms | Something being explained — a change that has to be followed: a clip travelling, a curve drawing on. Not for something merely entering or leaving (see phase 3: `app-fade-in`, which used to be this, is `--dur-2` now). |
| `--ease-out` | `cubic-bezier(0.2, 0.65, 0.3, 1)` | Anything arriving or responding. Decelerating: fast to start so it feels immediate, settling so it doesn't stop dead. |
| `--ease-in-out` | `cubic-bezier(0.4, 0, 0.2, 1)` | Anything moving between two on-screen positions. This is Tailwind's default, kept deliberately so the migration changes no timing that was already right. |

Three, not two, because "responds to my finger", "arrives on screen" and
"explains what happened" are genuinely three different jobs. A fourth
would be someone wanting a specific number, which is what this table
exists to prevent.

Phase 3 (below) adds exactly two more tokens, and says why they are not
durations:

| Token | Value | For |
|---|---|---|
| `--ease-spring` | `cubic-bezier(0.34, 1.56, 0.64, 1)` | Things that *land*: a tool badge's glyph, the logo's bars. The site's `back.out`: a small overshoot, and nothing that is not arriving uses it. |
| `--stagger` | `30ms` | The gap between siblings arriving in order. A gap, not a duration, so it is not a fourth timing: the site uses 40 to 70 ms and this is tightened for a tool. |

## Reduced motion

Honoured globally rather than per-component, because a per-component
opt-in is a list that is one component out of date the moment anybody
adds a component:

```css
@media (prefers-reduced-motion: reduce) {
  *, *::before, *::after {
    animation-duration: 0.01ms !important;
    animation-delay: 0s !important;
    animation-iteration-count: 1 !important;
    transition: none !important;
    scroll-behavior: auto !important;
  }
}
```

Two things changed in phase 3, both found by running the whole e2e suite
under `reducedMotion: "reduce"` for the first time:

- **Delays are zeroed.** A staggered reveal flattened to 0.01 ms but left
  with its delays would still hold each item back and pop them in one at
  a time, which is motion, only jerkier.
- **Transitions are off, not 0.01 ms.** The block used to say
  `transition-duration: 0.01ms`. An element's `transition-property` is
  `all` unless it says otherwise, so that made *every* property change on
  *every* element a transition, and a transition holds the old value until
  the next frame. Layout read in the same tick then sees the old box.
  Zoom-to-selection sets the timeline scrollbar's `scrollLeft` straight
  after its content's width changes; under reduced motion the width had
  not changed yet, the assignment was clamped to the old range, and the
  scrollbar and the lanes disagreed (`zoom.spec.ts` and
  `timeline-axis.spec.ts` failed, on the code before this phase too).
  Nothing listens for `transitionend`, so nothing is lost. Animations keep
  0.01 ms, because `usePresence` has an `animationend` fast path.

`0.01ms` rather than `0` for animations so `animationend` still fires —
anything sequencing off it keeps working instead of hanging. The
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

> **Correction (#236).** The four rows above describe the leave
> behaviour, and for a period they were false. #211 shipped the *arrive*
> half only — `overlay-in`, `backdrop-in`, `strip-in` — and all four
> surfaces still returned `null` the instant their flag flipped, so
> closing one remained the single-frame blink #211 set out to remove.
> This document said otherwise, in the column headed "What it does now",
> and the ticket was closed as completed against it.
>
> The leave half landed in #236: `usePresence` holds the mount open for
> one `--dur-2` while `overlay-out` / `backdrop-out` / `strip-out` play.
> The rows are accurate as of that change.
>
> The note stays rather than being quietly deleted. An audit that can be
> silently corrected is worth less than one that records when it was
> wrong — future work is planned against this table, and "it has been
> wrong before, here is how" is part of what it should say.

### Already correct — deliberately unchanged

| Component | Why |
|---|---|
| `ErrorBanner` | Already entered on `app-fade-in`. The ticket listed it as appearing instantly; that is not what the code did. **Phase 3:** it drops in from the edge above it (`banner-in`), as `KeyringWaiting` does |
| `MessageBubble` | Already fades in per message. Correct as-is, retimed to `--dur-2` with the rest of `app-fade-in`. **Phase 3:** its chips follow it in order |
| `EmptyState` | Already faded in as a whole. **Phase 3:** the logo builds and the groups arrive in order, once. See the phase 3 table |
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
| `McpServersEditor`, `SkillsEditor`, `AgentProfilesEditor`, `MemoryEditor` | Forms. Their hover and focus states are covered by the vocabulary migration; beyond that there is no change to explain |
| `RecentProjects`, `TranscriptPane` | Content surfaces whose changes are self-evident from the content itself. (Phase 3 gives the rows a lit edge on hover and the pane a fade when its tab is chosen, and nothing else.) |

> **Revisited in phase 3.** `Settings`, `ToolBadge`, `Chat` and
> `AppHeader` were in the two rows above, as "forms" and "content
> surfaces whose changes are self-evident". The owner asked for the app to
> move the way the site does, and on a second look each has one thing that
> is not self-evident, which is job 1 or 2 of the bar:
>
> - **Settings** opens like every other overlay, and its tab content fades
>   (job 2, job 1). The audit's objection was to animating a form's
>   fields, which still does not happen.
> - **ToolBadge** confirms a tool landed (job 2): the glyph settles and
>   the tint eases.
> - **Chat** shows what arrived: a plan that needs approval, its steps, a
>   notice, a chip (job 1).
> - **AppHeader**'s view tabs show where the view went (job 1).
>
> **Context menus stay unanimated**, exactly as the row above says.

| Waveform redraw | Same reason as the spectrum, and it is on the render path |

### Deferred, with a reason

| Component | Job | Why not here |
|---|---|---|
| `Timeline` / `ClipStrip` | 1 — the biggest one in the app | A cut or a move is *the* change that needs explaining. **Done in the follow-up**, and it needed no FLIP: the chips are already absolutely positioned by percentage, so the browser interpolates them directly |
| `GraphView` | 1 | Branching is the product's core idea and was silent. **Done in the follow-up** — node entry and an eased head ring |
| `ABCompareBar` | 1 | **Done in the follow-up**, for the visible switch. See the note below on what was *not* done |
| `AutomationLane` | 1 | **Done in #269** — a curve a tool writes draws itself on; see below |

### `AutomationLane`: done in #269, and how

The deferral said drawing on was ordinary and *when* was the hard part:
on a curve a tool has just written, never while a control point is being
dragged, never on first paint. Two things turned out differently.

- **When is one rule.** A curve draws on only if it arrives different
  from the one on screen. The first paint has nothing to differ from, a
  drag is already showing the dragged curve, and the refresh after the
  user's own edit brings back what they drew — so all three suppressions
  fall out of the one comparison, with nothing shared with the clip
  strip to build twice. "Different" allows for storage: the backend
  keeps times in samples and gains as `f32`, so an exact comparison
  would have animated every edit the user made.
- **Not `stroke-dashoffset`.** The lane draws with
  `vector-effect: non-scaling-stroke`, which puts dashes in screen space
  while the path's measured length is in user space. It reveals with a
  `clip-path` sweep instead, as a CSS animation (`--dur-3`,
  `--ease-out`), so the global reduced-motion block applies to it.

`e2e/automation.spec.ts` covers the four cases, including the one an
exact comparison gets wrong.

### A note on the A/B "crossfade"

The audit originally said a crossfade would make the comparison legible
as a comparison. That reads as one thing and is two:

- **The visible switch** — the A and B buttons changing state instantly,
  so the toggle did not read as a toggle. Done.
- **An audio crossfade between the two sides.** Not a motion change at
  all — it is playback work, and it was not claimed under this ticket.
  Done in #269 §2: the mix plays through two players, and a switch
  loads the new side on the idle one and crosses them over 60 ms
  (equal power) from the same moment. `e2e/ab-crossfade.spec.ts` reads
  it from the two `<audio>` elements in real playback.

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

## Phase 3 — the site's feel

The website moves (GSAP: a press dip, staggered reveals, a logo that
builds itself) and the app, which shares its brand since the violet-on-zinc
change, sat still beside it. This phase brings the *feel* across and
keeps the rules above: CSS only, the vocabulary above plus the two tokens,
every motion off under `prefers-reduced-motion`, and a "no" for anything
on the audio path.

**CSS only, again.** What the site needs GSAP for (ScrollTrigger, scrub,
DrawSVG on arbitrary paths, Flip, pointer-tracking) the app does not do.
What it does want is all expressible in CSS: DrawSVG is `pathLength="1"`
and a `stroke-dashoffset` keyframe, `back.out` is a cubic-bezier with an
overshoot, a stagger is an inline `--i` times `--stagger`, the link sweep
is a pseudo-element's `scale` with a flipped `transform-origin`. Nothing
was added to `package.json`.

**Mount-driven only.** An entrance is a class on an element that mounts.
No effect toggles a class, no timer runs, and app code never calls
`getAnimations()`. That keeps the motion invisible to jsdom (which
computes none) and to the slow-scheduler run, and adds nothing
asynchronous. The two things that are state rather than animation are
declared where they belong: the press `scale` only inside
`@media (prefers-reduced-motion: no-preference)`, so it is absent under
reduce rather than snapping, and a fader readout's `data-held`, which two
pointer handlers set.

### What moves

Feedback is `--dur-1` (120 ms), enter and exit `--dur-2` (200 ms), and
every stagger is capped so an entrance, delay included, stays under 300 ms.
Only `opacity`, `translate`, `scale`, `transform`, colours and (on small
elements) `box-shadow` are animated.

| Element | Trigger | Property | Duration | Easing |
|---|---|---|---|---|
| Buttons (`.btn-*`), pills, lane mute / solo | `:active`, not disabled | `scale: .96` | 120 ms in and out | `--ease-out` |
| Buttons, pills, ring-hover cards | `:hover` | colours, `border-color`, `box-shadow` | 120 ms | `--ease-out` |
| Primary button (`.btn-primary`) | `:hover` | `box-shadow` (a wider glow) | 120 ms | `--ease-out` |
| Text link (`.link-accent`) | `:hover`, `:focus-visible` | `::after` `scale` 0 to 1 from the left; out to the right on leave | 200 ms | `--ease-out` |
| Tab indicator (`.tab-indicator`): Settings tabs, view tabs | the tab becomes selected | `::after` `scale` 0 to 1 from the left | 200 ms | `--ease-out` |
| Settings card | mount | `overlay-in` (`opacity`, `scale .98` to 1) | 200 ms | `--ease-out` |
| Settings scrim | mount | `backdrop-in` (`opacity`); **no `backdrop-blur`** | 200 ms | `--ease-out` |
| Settings tab content | tab change (keyed remount) | `opacity` only (`fade-in-quick`) | 120 ms | `--ease-out` |
| Plan card | mount | `plan-in` (`opacity`, `translate` 6 px to 0) | 200 ms | `--ease-out` |
| Plan steps | with the card | `stagger-in` (`opacity`, `translate` 3 px), delay `min(i, 5) x 30 ms` | 120 ms each, 150 ms of delay at most | `--ease-out` |
| Tool badge glyph | status becomes done / error (keyed remount) | `badge-settle` (`opacity`, `scale .6` to 1) | 200 ms | `--ease-spring` |
| Tool badge tint | status change | `color`, `background-color`, `border-color` | 200 ms | `--ease-out` |
| Chat message, tool badge, node divider, plan history entry | mount | `app-fade-in` (`opacity`, `translate` 4 px) | 200 ms | `--ease-out` |
| Message chips | mount, at turn end | `stagger-in`, delay `min(i, 3) x 30 ms` | 120 ms | `--ease-out` |
| Banners (`ErrorBanner`, `KeyringWaiting`, `chat-error`, `chat-notice`) | mount | `banner-in` (`opacity`, `translate` -4 px to 0) | 200 ms | `--ease-out` |
| Transcript, Graph | the tab is chosen | `fade-in` (`opacity`) | 200 ms | `--ease-out` |
| Fader readout | the slider is held | `color` to `--accent-text` | 120 ms | `--ease-out` |
| Selection overlay | mount | `fade-in-quick` (`opacity`); `left` / `width` never | 120 ms | `--ease-out` |
| Empty state: logo bars | mount | `logo-bar-in` (`transform: scaleY(.15)` to 1), 40 ms apart | 200 ms | `--ease-spring` |
| Empty state: logo strokes | mount, after 80 ms | `logo-line-draw` (`stroke-dashoffset` 1 to 0) | 200 ms | `--ease-in-out` |
| Empty state: groups | mount | `reveal-in`, delay `min(i, 3) x 30 ms` | 200 ms, 90 ms of delay at most | `--ease-out` |
| Ring-hover cards (recent projects, templates, hint chips) | `:hover` | `border-color`, `box-shadow`; no lift | 120 ms | `--ease-out` |

Also: `app-fade-in` moved from `--dur-3` to `--dur-2`, and `wave-pulse`
(the ten bars that pulsed on the empty state for ever, which kept an idle
window repainting) is deleted with its `@keyframes`; the bars became the
static logo mark in the brand pass and this removes what was left of them.

### What does not move, and why

Repeated here because the next person will want to "fix" it, and because
`styles.css` carries the same list above the motion block.

- **Anything WaveSurfer draws, or any ancestor of it**:
  `timeline-lane-waveform`, `-surface`, `timeline-lane`, `timeline-body`,
  `timeline-root`, `<main>`. An animated ancestor promotes the canvases to
  a layer that is composited, resized and repainted under them. This is
  why `<main>` and `timeline-root` no longer carry `app-fade-in`, and why
  the Timeline, unlike the Transcript and the Graph, has no fade when its
  tab is chosen. `e2e/motion.spec.ts` walks from the waveform container
  to `<html>` and fails on any animation, running transition or
  `will-change` on the way, after load, with Settings and a plan card
  open, and during playback.
- **The playhead** (moved every frame by `left`: a transition would lag
  behind the audio), **a selection's `left` / `width`**, the ruler, the
  spectrum, marker positions, clip and label drags, an automation point
  being dragged.
- **A lane as a unit.** Never key, remount or hold a lane open to animate
  it: the leaving lane would keep its WaveSurfer and its `<audio>` alive
  beside the new one, which is the shape of the Linux freeze (#455). And
  add no `<audio>` / `<video>` for a UI effect.
- **`filter`, `backdrop-filter`, layout properties.** The Settings scrim
  had a `backdrop-blur-sm` under an opacity animation; WebKitGTK re-blurs
  the whole window every frame for that, so it is gone and the scrim is
  `bg-black/60`.
- **Anything infinite on an idle screen.** `animate-spin` and
  `animate-pulse` stay only where something is busy (the running glyph,
  the caret, the send button while it sends).
- **Anything per streamed token.** A reply's bubble animates once, when it
  mounts; the text growing in it is not animated, and
  `e2e/motion.spec.ts` counts the page's animations across a stream.
- **A press on a drag handle.** The `scale` dip is on the button recipes,
  pills and `[data-press]` only. Range inputs, clip and label chips, the
  automation points, React Flow nodes and menu items do not match, and a
  handle that shrinks under the pointer mid-drag moves its own hit target.

### Not done

- **Settings leaving.** Closing the dialog is still instant. An exit
  animation needs the dialog to stay mounted for 200 ms after it is
  closed, and the usual way here (`usePresence`) holds the *same* subtree:
  every one of its roughly sixty test ids, its fields and its focus would
  stay in the document and in the accessibility tree for that long, and
  the tests that assert the dialog is gone the instant it closes would
  race it. The alternative is a visual ghost (a cloned node with the ids
  and roles stripped and `inert`), which is imperative DOM work outside
  React for a dialog that is closed once per session. Neither is simple
  or test-safe, so Settings closes at once, as it always has.
- **The plan card leaving.** It comes down at once on Run and Discard (a
  test holds it to that); the plan history entry arriving is the motion.
- **A toast system.** There are none in the app and none was added;
  banners leave instantly, because tests assert they are gone.

### How it is tested

`motion.test.tsx` and the component tests check classes (jsdom computes no
animation, so a test claiming to observe one would pass against nothing),
the stylesheet (every `animation:` has a `--dur-N`, every keyframe animates
only cheap properties, the press is inside the no-preference block, delays
are zeroed under reduce) and, as source guards, that `<main>` and
`timeline-root` carry no motion class. `e2e/motion.spec.ts` plays each in
Chromium and reads `getAnimations()` and computed style, never sleeping;
it also runs the same elements under `reduce`. The whole e2e suite runs
under a global `reducedMotion: "reduce"` so nothing else waits on an
entrance; `automation.spec.ts` and `motion.spec.ts` opt back in.

## What this does not do

No decorative motion was added. Nothing was animated because it could be.
The four deferred items are the ones with real value left in them, and
they are deferred because they are a different piece of work, not because
they are optional.
