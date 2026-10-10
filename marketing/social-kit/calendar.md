# Two-week posting plan

Starts Monday 12 October 2026, two days after v0.4.0 was published (9 October).
Every piece is in this folder: `linkedin-posts.md`, `instagram.md`,
`reddit.md`, `more.md`. Videos are the `reel-*.mp4` files (and their
`-narrated` versions and `.srt` captions).

I have no data on when your audience is online, so no times of day are given.
Pick a slot in your own time zone and keep it the same each day. The weekday
choices for Show HN and Reddit are conventional wisdom, not measurements.

## Principles

- **Lead with the strongest reel.** `reel-highlights.mp4` goes out first,
  everywhere your own followers are. The single-demo reels follow it, one per
  few days, so the feed does not show the same video twice in a row.
- **One community post that needs you present per day.** Show HN and each
  Reddit post need you in the thread for the first several hours. Never stack
  two of them. Own-feed posts (LinkedIn, Instagram, X) are lighter and can
  share a day with one of them.
- **Show HN before Reddit.** HN finds the problems fast. Fix or answer them
  before the technical subs see the project (r/rust, r/LocalLLaMA).
- **Each post lands on a different audience with different text.** Do not
  paste the same text twice.
- **Nobody is asked to upvote, star or share.**

## Before day 1 (do these this weekend)

- [ ] Confirm the five `reel-*.mp4` files, the `-narrated` versions and the
      `.srt` files exist, play with sound, and match the captions (see the
      note at the bottom of `instagram.md`: trim any caption detail the final
      edit cuts).
- [ ] Install v0.4.0 on one machine per OS you will claim. Check the macOS
      `xattr` step in the release notes works as written.
- [ ] Set the Instagram bio link and bio line (`instagram.md`).
- [ ] Decide the Claude Code disclosure (`linkedin-posts.md` post 3 and the
      HN, r/rust and r/opensource posts include it; keep it).
- [ ] Add the `good first issue` label to two small issues, so the r/opensource
      and LinkedIn #5 posts can name real ones. None is labelled today.
- [ ] Read the sidebar and rules of r/rust, r/LocalLLaMA, r/DJs,
      r/opensource, r/edmproduction and r/WeAreTheMusicMakers. Write down
      which of them restrict self-promotion; move or drop those posts.
- [ ] Open <https://edytlab.com/#demos> on a phone and play a demo.
- [ ] Fix the analytics contradiction before anyone quotes it:
      `website/app/privacy/page.tsx` says the website uses anonymous Vercel
      analytics, while `marketing/` drafts say it has none. The new posts
      avoid the claim.

## Week 1

| Day | Piece | Where | Video | File and section |
|---|---|---|---|---|
| **Mon 12 Oct** | The launch post | LinkedIn | `reel-highlights.mp4` | `linkedin-posts.md` #1, then pin its comment |
| | The lead reel | Instagram, TikTok, YouTube Shorts | `reel-highlights.mp4` | `instagram.md` #1; `more.md` section 4 |
| | The thread | X | `reel-highlights.mp4` | `more.md` section 1 (thread) |
| | Short versions | Bluesky, Mastodon | `reel-highlights.mp4` | `more.md` sections 2 and 3 |
| | Announcement | Newsletter and Discord | link only | `more.md` section 7 |
| **Tue 13 Oct** | Show HN, then stay 4 to 6 hours | Hacker News | none (submit the site) | `more.md` section 6 |
| | Story frame 3 | Instagram stories | still from `reel-highlights.mp4` | `instagram.md` story frames |
| **Wed 14 Oct** | Engineering notes | r/rust | none (text) | `reddit.md` #6 |
| | The beatmatch reel | Instagram, TikTok, Shorts | `reel-beatmatched-transition.mp4` | `instagram.md` #2; `more.md` section 4 |
| **Thu 15 Oct** | Local-model data request | r/LocalLLaMA | none (text) | `reddit.md` #5 |
| | The build story | LinkedIn | `reel-mini-mix.mp4` | `linkedin-posts.md` #3, then pin its comment |
| **Fri 16 Oct** | The intro reel | Instagram, TikTok, Shorts | `reel-extended-club-intro.mp4` | `instagram.md` #3; `more.md` section 4 |
| | Single post | X | `reel-teaser.mp4` | `more.md` section 1 (single post) |
| **Sat 17 Oct** | Full recordings with chapters | YouTube (long form) | the three `website/public/demos/*.mp4`, back to back | `more.md` section 5 (check the chapter times) |
| **Sun 18 Oct** | No posts. Answer comments, file issues for what people found, fix anything the posts got wrong, label two good first issues | GitHub | | |

## Week 2

| Day | Piece | Where | Video | File and section |
|---|---|---|---|---|
| **Mon 19 Oct** | DJ show-and-tell | r/DJs | `reel-beatmatched-transition.mp4` (or text) | `reddit.md` #1 |
| | Before and after | LinkedIn | `reel-beatmatched-transition.mp4` | `linkedin-posts.md` #2, then pin its comment |
| | Story frame 1 (question sticker) | Instagram stories | still from `reel-highlights.mp4` | `instagram.md` story frames |
| **Tue 20 Oct** | Contributors wanted | r/opensource | none (text) | `reddit.md` #4 |
| **Wed 21 Oct** | The mini-mix reel | Instagram, TikTok, Shorts | `reel-mini-mix.mp4` | `instagram.md` #4; `more.md` section 4 |
| | DJ-friendly edits question | r/edmproduction | `reel-extended-club-intro.mp4` | `reddit.md` #3 |
| **Thu 22 Oct** | The privacy post | LinkedIn | `reel-extended-club-intro.mp4` | `linkedin-posts.md` #4, then pin its comment |
| | Which audio chore? | r/WeAreTheMusicMakers | `reel-extended-club-intro.mp4` (or text) | `reddit.md` #2 |
| **Fri 23 Oct** | Open source, help wanted | LinkedIn | `reel-teaser.mp4` | `linkedin-posts.md` #5, then pin its comment |
| | The teaser reel | Instagram, TikTok, Shorts | `reel-teaser.mp4` | `instagram.md` #5; `more.md` section 4 |
| **Sat 24 Oct** | Optional follow-up: "what changed after your feedback", only if something did | LinkedIn, X, Bluesky | a screen capture of the fix, if any | write fresh; do not invent changes |
| **Sun 25 Oct** | Review: GitHub release downloads, stars, issues opened. Decide what to post next | | | |

## Why this order

- **Mon, the highlights reel first.** It carries all three demos, so a
  newcomer sees the whole range in one video. It is also the video most likely to
  hold a viewer who has not heard of the project.
- **Tue, Show HN.** It is the most scrutiny-heavy post, and `more.md`'s
  version states the limits first. Everything technical that follows has
  already been through it.
- **Wed and Thu, r/rust and r/LocalLLaMA.** The two audiences that will test
  specific claims (vocoder, lock discipline, local-model numbers). Both posts
  ask for review, not downloads.
- **Weeks 1 and 2, single-demo reels one at a time.** Each lands on a day with
  no community thread to babysit.
- **Week 2, the music communities.** r/DJs, r/edmproduction and
  r/WeAreTheMusicMakers are the likeliest to restrict self-promotion, so they
  come after you know how the project reads to strangers, and after you have
  checked each sidebar. Drop any that do not allow it.
- **Week 2, open source last.** By then you have labelled starter issues and
  fixed what week 1 found, so the invitation has something real behind it.

## Keep a record

Copy this into a note and tick as you go.

| Piece | Posted (date) | Link | Replies answered | Issues opened because of it |
|---|---|---|---|---|
| LinkedIn #1 | | | | |
| Instagram, TikTok, Shorts: highlights | | | | |
| X thread | | | | |
| Show HN | | | | |
| r/rust | | | | |
| r/LocalLLaMA | | | | |
| LinkedIn #3 | | | | |
| r/DJs | | | | |
| r/opensource | | | | |
| r/edmproduction | | | | |
| r/WeAreTheMusicMakers | | | | |
| LinkedIn #2, #4, #5 | | | | |

## Measuring

The only built-in signals are GitHub release download counts, stars and
issues. There is no in-app telemetry, by design, so do not add any for the
launch. The platforms' own insights cover views and watch time. Do not
publish any number from these as a claim unless you read it off the live
page the day you post it.

## If a claim turns out to be wrong

Correct it in the same thread, plainly, and fix the source file here. The
claims most likely to be questioned: the synthetic demo tracks, the scripted
recording, the unsigned installers, what local models can do, and the
"Master" track that bakes the mix. All five are disclosed in the posts.
