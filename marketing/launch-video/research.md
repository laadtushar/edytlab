# Research: launch videos for LinkedIn, Reddit and Instagram, and the motion design behind the cut

Researched 2026-10-10. Every row gives the page, the line quoted from it, and a status:

- **read**: I opened the page and the line is quoted from what it returned.
- **secondary**: a reputable summary of a primary source I could not open; the primary is named.
- **unverified**: could not be confirmed from anything I could open. Not relied on.

Where a page could not be opened (Reddit's own help pages and subreddit rules return 401/403 or "unable to fetch" from this
environment; Instagram's help pages are JavaScript-only), the row says so instead of guessing.

---

## A. LinkedIn

| Claim | Source | Quoted line | Status |
|---|---|---|---|
| Native video accepts four aspect ratios | LinkedIn Video Ads specs, <https://business.linkedin.com/advertise/ads/sponsored-content/video-ads/specs> | "Ratio: 4:5 (vertical, 0.8), 9:16 (vertical; 0.57), 16:9 (landscape; 1.78), 1:1 (square; 1.0)" | read |
| Frame rate, container, captions | same | "Recommended frame rate: 30 frames per second"; "Video File Type: MP4"; "Video Captions: Optional" | read |
| Square is supported up to 1920x1920; 16:9 recommended size is 1080p | LinkedIn Help, Video ads specifications, <https://www.linkedin.com/help/linkedin/answer/a424737> | "Horizontal 16:9 (1.78): Recommended 1920 x 1080 pixels"; "Square 1:1: ... Maximum: 1920 x 1920 pixels" | read |
| Ads: short is recommended; captions must be SRT | same | "We recommend uploading videos that are 15-30 seconds long."; captions "must be in SRT format ... Custom formatting (for example, font colors) isn't supported." | read (ad spec; organic posts differ) |
| Organic video limits | LinkedIn Help, Video sharing troubleshooting, <https://www.linkedin.com/help/linkedin/answer/a548372> | "Maximum file size: 5 GB"; "Maximum video duration: 15 minutes"; "Aspect ratio: 1:2.4 - 2.4:1" | read |
| Captions are attached as an SRT in the desktop flow | LinkedIn Help, Add closed captions, <https://www.linkedin.com/help/linkedin/answer/a552177/add-closed-captions-to-videos-on-linkedin?lang=en> | "Click **Select Caption** to attach a SRT file and confirm your selection." | read |
| Most people watch muted, so burn the words in | LinkedIn Marketing Solutions, video ad tips, <https://business.linkedin.com/marketing-solutions/success/best-practices/video-ad-tips> | "Think like a silent film director: a large portion of LinkedIn members will watch your ad with the sound off."; "Consider burning in video subtitles." | read |
| Show the point early | same | "Show what you want your audience to see in the first 10 seconds of the video."; "Establish your point in the first 5 seconds, then drive it home." | read |
| Shorter finishes more often | same | "Short-form videos are more likely to be watched to completion."; "7-15 second videos seeing up to a 300% lift in completion rates" (attributed to "LinkedIn data, 2025") | read (LinkedIn's own figure, ads) |
| A clear call to action | same | "Feature a clear CTA (call to action), so your audience knows how to act on their interest." | read |
| Muted viewing, mobile, and a hook | LinkedIn blog, 13 tips for B2B video (published June 5, 2025), <https://www.linkedin.com/business/marketing/blog/content-marketing/13-top-tips-for-compelling-b2b-video-content-on-linkedin> | "Research shows that 92% of people watching videos on mobile devices do so without sound."; tip 11 "Make the first six seconds count"; "However, the optimum length is usually much shorter." | read |
| Vertical lifts clicks on mobile (ads) | same | "vertically-oriented videos (9:16) are showing a 24% lift in click-through rates" | read (ad CTR, not organic reach) |
| Not a talking head | same | "B2B video content should never just mean another talking-head interview." | read |
| Founder-voice framing helps | no first-party source found; the article above has no founder-specific advice | the cut uses "I built an audio editor you talk to." only because the owner's existing LinkedIn post 1 (`marketing/social-kit/linkedin-posts.md`) opens that way | **unverified** as a performance claim |

**What this decided for the LinkedIn cut:** MP4/H.264/AAC at 30 fps; burned-in text because the feed is muted; the first frame already says
what the thing is; one clear call to action; 69 s total (organic limit is 15 min; LinkedIn's own "short" advice is for ads, so the cut
puts the whole point in the first 10 s and lets the proof run after). **Square 1:1 (1080x1080)** rather than 16:9: LinkedIn lists 1:1
as supported, mobile is where people watch, a square takes more of a phone feed than a 16:9 strip, and the app's chat panel is
near-square, so zoomed shots fill the frame. LinkedIn publishes no preference between 1:1 and 4:5 for organic video; this is a judgement call
(see README, decisions).

---

## B. Reddit

| Claim | Source | Quoted line | Status |
|---|---|---|---|
| Reddit video upload limits (1 GB, 15 min) | third-party guides (dacast, capcut, zight); Reddit's own help article could not be opened (401/403) | "Videos can be up to 1 GB." / "The maximum video length is 15 minutes." | **unverified** (secondary only) |
| Reddit ad specs | <https://business.reddithelp.com/en/categories/campaign-setup/reddit-ad-unit-specifications> | HTTP 401 | **unverified**; not needed, the cut is organic |
| Conversational, in-use beats polished | ppc.land summary of Reddit's 2026 ad creative study, <https://ppc.land/reddits-2026-ad-creative-study-what-actually-moves-conversion-rates/> | "Showcasing products 'in their natural setting' outperformed ads with 'solid or colourful backgrounds on average.'"; "a high-budget product shot on a colour-matched background performed worse"; headlines "should be written to sound like something a real person would say when read aloud." | secondary (Reddit's study, via a news summary) |
| Overlay text and captions help | same | "video assets using overlay text saw 8.2% higher average conversion rates than those without" | secondary |
| Short wins in ads | same | "Assets under 6 seconds saw the highest average conversion rate impact." (does not cover a 60 s organic post) | secondary; **not applied** to the organic cut |
| r/DJs on self-promotion | only a 2019 mirror of the rules post, <https://lr.vern.cc/r/DJs/hot> | self-promotion of mixes/tunes/companies is removed; regulars get more leeway | **unverified** (2019, mirror) |
| r/WeAreTheMusicMakers, r/opensource rules | `reddit.com/r/<sub>/about/rules(.json)` returned 403 / "unable to fetch" | none | **unverified** |
| Reddiquette "10 %" self-promotion guideline | Reddit help page returned 403; the figure is widely quoted by mirrors | none | **unverified** |

**What this decided for the Reddit cut:** 16:9 1920x1080 (the recording is landscape, Reddit's feed shows it full width), no founder
hook, no hype words, a plain first line ("edytlab: an open-source audio editor you drive in plain English"), real screen recording
from the first second of the demo, the limits stated on screen, and an end card whose only call to action is the GitHub address. Rules of
each subreddit **must be re-read on the day**; `marketing/social-kit/reddit.md` already says the same. Reddit video posts have no body, so the post text
goes in the first comment.

---

## C. Instagram Reels

| Claim | Source | Quoted line | Status |
|---|---|---|---|
| Hook inside 3 seconds | Instagram for Creators FAQ, <https://creators.instagram.com/faq?locale=en_US> | "Make sure the first 3 seconds of your reel are engaging, so that people don't move on." | read |
| Trending audio can affect reach | same | "Using trending audio can also impact distribution." | read |
| Originality is rewarded | same | "We encourage people on Instagram to post content that is original to get the best reach and distribution." | read |
| Length for non-followers | same | "We recommend videos to unconnected audiences that are 3 minutes or less." | read |
| What ranking weighs | same | "the most important things we consider are how much engagement per viewer the content has received" | read |
| Reels max length | <https://about.instagram.com/features/reels> | "Create multi-clip videos up to 3 minutes." (the Help Center says up to 20 minutes and "Reels over 3 minutes won't be recommended to new audiences.") | read; the two pages disagree |
| 9:16 at 1080x1920 | Meta Business, Reels video ad specs, <https://www.facebook.com/business/ads-guide/update/video/instagram-reels> | "Ratio: 9:16"; resolution listed as "1440 x 2560 pixels"; "Video Sound: Optional, but strongly recommended" | read (ad spec; 1080x1920 is the commonly cited minimum for organic and is within the range) |
| Organic upload range | Instagram Help, Reel size & aspect ratios, <https://help.instagram.com/1038071743007909> | page is JavaScript-only; a search snippet gives "between 1.91:1 and 9:16", minimum 30 fps and 720 px | secondary |
| Safe zone | Meta Business, same spec page | "Consider leaving at least 14% of the top, 35% of the bottom, and 6% on each side of your asset free from text, logos, or other important creative elements." | read (stated for **ads**) |
| Why | Meta Business Help, <https://www.facebook.com/business/help/980593475366490/> | "keep the edges (top, bottom and sides) free of key creative elements, text and logos."; "so ... aren't cropped out or covered by the user interface, such as the profile icon or a call to action"; with disclaimers "leave the bottom 40% of your ad free" | read |
| Organic-reel safe zone | no first-party figure found; third-party guides disagree (bottom 250 px to 35 %) | none | **unverified**; the cut uses Meta's ad numbers, the strictest published |
| Captions: auto-captions exist and can be managed | <https://help.instagram.com/7487270478066359/> | page is JavaScript-only | **unverified**; text is burned in so the cut does not depend on it |

**What this decided for the Instagram cut:** 1080x1920, 30 fps, 31 s (inside 3 minutes; the whole point lands in the first 3 s);
the hook line is on screen from frame 3; all text and the frame sit inside x 65-1015 and y 269-1248 (14 % top, 35 % bottom, 6 % sides);
the only audio is the real app audio, so there is no trending-audio dependency and the post is "original audio". The 30-second narrated
version uses the owner-approved voice and must carry the ElevenLabs credit (README).

---

## D. Launch-video structure

| Claim | Source | Quoted line | Status |
|---|---|---|---|
| Problem, solution, result | Vidyard, <https://www.vidyard.com/chalk-talks/video-marketing/using-video-to-crush-your-next-product-launch/> | "Present a problem, then the solution, then the results that the solution created." | read |
| Show it | same | "Show it! This is a video, after all."; "Use in-video CTAs to drive viewers to valuable activities and other conversion points." | read |
| Under two minutes | same | "As a rule of thumb, aim for under two minutes." | read |
| Teasers under a minute; demos 1-5 min | Wistia, <https://wistia.com/learn/marketing/optimal-video-length> | "If your goal is to boost visibility and stop the scroll on social media, shorter is better."; demos "1-5 mins": "The 1-5 minute range is the sweet spot." | read |
| Say the point fast | same | "Make sure your short video gets to the point fast."; for 1-5 min "include the most important or compelling information in the first half of your video." | read |
| Product on screen early | Wistia, <https://wistia.com/learn/marketing/product-video-best-practices> | "make sure the product appears early on in the video so that viewers know exactly what to expect"; "Demonstration videos do exactly what the name says—they use video to illustrate the product in action." | read |
| Win the viewer early | Wistia, <https://wistia.com/learn/marketing/understanding-audience-retention> | "if you don't win me early, you're not going to keep me."; "Get to the point, already!" | read |
| Shorter engages more | Wistia State of Video, quoted on <https://wistia.com/learn/marketing/video-marketing-statistics> | "The shorter the video, the higher the engagement rate." | read |
| Short-form video ROI; 30-60 s preferred | HubSpot, <https://www.hubspot.com/marketing-statistics> | "short-form video (49%)" tops ROI formats; "51% of people say that the optimal length for an effective video is 30-60 seconds." (Wyzowl 2026) | secondary |
| Unpolished is fine if it is real | Y Combinator, <https://www.ycombinator.com/library/J8-yc-application-tips-include-a-demo> | "We don't care if the demo is unpolished. We don't care if you're still running it locally or if it's just a CSV right now." | read (transcript on the page) |
| Product Hunt launch guidance (30-60 s video) | Product Hunt's own guide returned 403 | one 2026 third-party guide says 30-60 s | **unverified** |

**Structure used (master, 63-69 s):** hook (what it is) -> problem (the chores) -> demo in six numbered steps on real footage -> proof
(real audio, real numbers, the agent flagging its own edge case) -> how it works (a labelled diagram) -> honest limits -> call to
action. 15/30/60 s cuts: the 31 s Instagram cut is hook + steps 1, 2, 3, 4 (audio), 5 + end card; the 63 s Reddit cut drops the founder
hook and the chores; the 69 s LinkedIn cut is the full master.

---

## M. Motion design, kinetic typography, UI zoom and callouts (what the code in `tools/engine.py` implements)

The owner asked for strong launch-video motion: kinetic typography, smooth zooms onto the agent's chat and the waveform, animated
callouts and consistent brand motion, driven by code. **Tooling:** I evaluated Remotion and Motion Canvas. Both need a headless
Chromium (not installed here; the npm registry and PyPI answer, and the Chromium download host answered with 403 on its index, but the install is hundreds of MB on a 6 GB disk with a 3 GB floor, on a
machine already at load 8-14, so I did not try it), and Remotion's licence terms for for-profit use could not be confirmed
(<https://www.remotion.dev/docs/license> returned only its intro). Instead the same ideas are implemented offline in about 1,000 lines of
Python (Pillow + ffmpeg, both already installed): a frame is a pure function of time, easing curves and springs are the ones the
libraries document, and ffmpeg encodes. Nothing was downloaded.

| What I learned | Source | Quoted line | Used as | Status |
|---|---|---|---|---|
| Entering things decelerate, leaving things accelerate; exact curves | Material Components motion docs, <https://github.com/material-components/material-components-android/blob/master/docs/theming/Motion.md> | "motionEasingEmphasizedDecelerateInterpolator ... cubic-bezier: 0.05, 0.7, 0.1, 1 ... animations that enter the screen"; "...EmphasizedAccelerate... cubic-bezier: 0.3, 0, 0.8, 0.15 ... exit the screen"; "motionEasingStandard... cubic-bezier: 0.2, 0, 0, 1" | `EASE_IN` for titles, callouts, frames arriving; `EASE_OUT` for exits (0.24 s) | read |
| Duration scales with distance | same | "In general, duration should increase as the area/traversal of an animation increases."; durations "motionDurationShort4 200ms ... Long2 500ms" | text reveals 0.55 s, callout ring 0.55 s, camera moves 0.8-1.1 s, exits 0.24-0.3 s | read |
| Camera tween default | Motion Canvas, <https://motioncanvas.io/docs/tweening/> | "By default, property tweens use easeInOutCubic as the timing function." | `EASE_CAM` (cubic-bezier .645,.045,.355,1) for every zoom and pan | read |
| Springs for settle | same; Remotion, <https://www.remotion.dev/docs/spring> | spring "using Hooke's law"; "A physics-based animation primitive."; damping: "How hard the animation decelerates."; "To disable the default bounce, increase the damping parameter." | callout chips and the URL pill use a damped spring, zeta 0.78 (about 2 % overshoot), not a bouncy default | read |
| Zoom to where the action is, not everywhere | Screen Studio, <https://screen.studio/guide/auto-zoom> | "The Auto zoom option focuses on the areas where clicks occurred during your recording." | zooms go to the composer when the request is typed, to the chip and answer when the tool runs, to the waveform when it is heard | read |
| Zoom and pan to direct attention | TechSmith Camtasia, <https://www.techsmith.com/learn/tutorials/camtasia/animations/> | "Zoom in or out on important parts of your video to draw attention to key actions."; "Use animations to zoom in to focus the viewer's attention and pan to important parts in your video." | same | read |
| Callouts: arrows and highlights for specific targets; one look across the video | TechSmith, <https://www.techsmith.com/learn/tutorials/camtasia/annotations/> | "Add a callout to display onscreen text, titles, and lower-thirds"; "Add arrows to point out areas"; a theme gives "a consistent look, style, or brand across videos." | ring + leader line + chip, one style for all three cuts, 2-4 per clip | read |
| Keep callout text short and use them sparingly | TechSmith blog, <https://www.techsmith.com/blog/annotate-recorded-calls/> | summary: on-screen text "around five to eight words"; do not annotate every minute | callout labels are 3-6 words | secondary (search summary) |
| Prepare the viewer, direct attention | Wikipedia, Twelve basic principles of animation, <https://en.wikipedia.org/wiki/Twelve_basic_principles_of_animation> | "Anticipation is used to prepare the audience for an action"; staging: "Its purpose is to direct the audience's attention, and make it clear what is of greatest importance in a scene." | the camera arrives before the callout draws (anticipation); one emphasised thing per shot (staging) | read |
| Reading speed for on-screen words | Netflix English Timed Text Style Guide, <https://partnerhelp.netflixstudios.com/hc/en-us/articles/217350977-English-Timed-Text-Style-Guide> | "Adult programs: Up to 20 characters per second."; "42 characters per line."; "Maximum two lines." | every title is two lines at most and stays up long enough: the longest (44 characters) holds 4 s, 11 characters per second | read |
| No flashing | WCAG 2.2, 2.3.1, <https://www.w3.org/WAI/WCAG22/Understanding/three-flashes-or-below-threshold.html> | "Web pages do not contain anything that flashes more than three times in any one second period" | all transitions are cross-fades of 0.3 s or more; no strobing | read |
| Text contrast | WCAG 2.2, 1.4.3, <https://www.w3.org/WAI/WCAG22/Understanding/contrast-minimum.html> | "a contrast ratio of at least 4.5:1" (3:1 for large text) | white on the background gradient is 17-20:1, the orange accent 7-8:1, the muted note text 8-9:1 (computed with the WCAG formula over the gradient's range, (8,9,13) to (24,27,40)) | read |
| Kinetic type: legibility first, few animated elements at a time, test muted | vendor blogs: <https://hera.video/blog/kinetic-typography-video-generator-guide>, <https://www.nemovideo.com/blog/kinetic-typography-product-ads> | summaries only: limit simultaneous animated elements; "fast motion for the hook, medium motion for benefits, a clear pause on the main claim, and the longest hold on the call to action" | titles reveal word by word but never rotate, scale hard or blur for long; the end card holds longest | secondary (vendor blogs; low weight) |

**The motion kit (same on all three cuts):**
1. **Easing**: enter on emphasized decelerate, exit on emphasized accelerate, camera on easeInOutCubic, settle on a damped spring.
2. **Kinetic titles**: each word rises 0.42 em, fades and de-blurs over 0.55 s, 70 ms apart; the accent word is orange. Step titles share one size per canvas.
3. **Camera**: a crop rectangle on the recording and the frame rectangle on the stage are interpolated together (centre linearly, size geometrically so the zoom
   speed looks constant), with a 1.2 % slow drift during holds so a held shot never looks frozen. Zoom is capped at 2.15-2.3x because the source is 1280x776.
4. **Callouts**: the ring draws itself on over 0.55 s (a trimmed rounded-rectangle path, anti-aliased by 3x supersampling), pulses at 0.9 Hz, sends one ripple,
   a leader line grows to a chip that springs in. Regions are in source pixels, so they follow the camera; where the recorded chat scrolls, the region is keyframed.
5. **Brand**: orange (242,140,60) from the app, the "edyt" serif italic + "lab" wordmark from the app's own title screen, a row of breathing bars (the app's
   logo motif), a thin orange progress line on every cut.
6. **Sound**: a synthesised soft whoosh on scene changes, a tick when a callout lands, a chime under the wordmark (`tools/sfx.py`, no samples, no licence).

---

## E. Things I could not confirm

- Reddit's own file-size/length limits, any subreddit's current rules, Reddiquette's wording.
- Any first-party evidence that founder-voice framing raises LinkedIn performance.
- Instagram's organic safe-zone numbers (Meta publishes them for ads only).
- Product Hunt's video-length guidance.
- Remotion's licence terms for a for-profit user (irrelevant here, nothing from Remotion is used).
- Whether Instagram's Help Center and the Reels page agree on the maximum length (they do not).
