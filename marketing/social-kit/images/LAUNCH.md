# Using the images for a launch

Read with [`../calendar.md`](../calendar.md) (the two-week plan: it is for 12 October, so shift its days),
[`../../launch/`](../../launch/) (Show HN, Product Hunt, Reddit) and [`../more.md`](../more.md) (X, Bluesky, TikTok,
Shorts). This file only says which image goes where and when; the words are in [`captions.md`](captions.md)
and those files. Where the calendar and this file differ, the calendar's rules win: one live community post
that needs you per day, Show HN before Reddit, a different text on each post, nobody asked to upvote, star or share.

## Which canvas, image or video, and where the link goes

Video-first platforms post the matching **reel** and use the 1080x1920 image as its cover. Every other platform
posts the image, with the video page in the text. File: `out/<theme>__<canvas>.png`.

| Platform | Post | Canvas | Link goes |
|---|---|---|---|
| Instagram feed | image (portrait) | `portrait-1080x1350` | not clickable: "link in bio", and set the bio link (with UTM) |
| Instagram Reels / Stories | the reel; image is the cover or a story | `story-1080x1920` | bio link; a link sticker on a story |
| TikTok | the reel; image is the cover | `story-1080x1920` | bio link (no clickable links in captions) |
| YouTube Shorts | the reel; image is the cover if the uploader offers one | `story-1080x1920` | description (pin a comment with it) |
| YouTube (long form) | the demo video; image is the thumbnail | `youtube-1280x720` | first line of the description |
| X | image + text | `x-1600x900` | in the post text |
| Bluesky | image + text | `x-1600x900` | in the post text (a link card may replace the image: attach the image first) |
| Threads | image (square or portrait) | `square-1080x1080` | in the post text |
| LinkedIn | image, or the video with the image as its thumbnail | `linkedin-1200x627` | first comment, not the post body (the calendar pins a comment) |
| Facebook | image + short paragraph | `square-1080x1080` (a link post uses `linkedin-1200x627`) | in the text |
| Pinterest | pin | `pinterest-1000x1500` | the pin's destination URL field |
| Product Hunt | gallery | `youtube-1280x720` (see below) | the Website field |
| Show HN, Reddit | text, no image | none | the post URL (HN) or the text (Reddit) |

## Order of the six themes

The themes are for your own feeds; community posts keep the calendar's order and run one a day on their own days.
Pair a theme with a community post only where it fits.

| Day | Own feeds | Aimed at | Image / video | Community post that day |
|---|---|---|---|---|
| Week 1 Tue | **1 Describe the edit** | everyone | launch video (`edytlab.com/videos/launch-video`), reel as cover | none: answer comments |
| Week 1 Wed | **2 A DJ transition in one sentence** | DJ communities | `reel-beatmatched-transition` | Show HN at 12:00 UTC, then stay 4 to 6 hours |
| Week 1 Thu | **4 Your audio stays on your computer** | privacy and local-AI audiences | image only (no video of its own) | r/rust (engineering notes) |
| Week 1 Fri | none, a reply round | | | r/LocalLLaMA (after Show HN and r/rust) |
| Week 2 Tue | **3 Extend a club intro** | DJs and producers | `reel-extended-club-intro` | Product Hunt at 12:01am PT (its own day) |
| Week 2 Wed | **5 Every edit is a version** | producers, editors | `whats-new-v070` (video page) | r/DJs or r/edmproduction |
| Week 2 Thu | **6 Free and open source** | developers, GitHub | image only | r/opensource, if the sidebar allows; link GitHub, not a reel |

Own feeds post Tuesday to Thursday only: weekends are the weakest days on every platform in the sources below.
The interactive version of this plan, with the schedule in your own time zone and copy buttons on every caption, is
the launch-kit artifact the owner was given; this file is the copy that lives with the images.

Day 1 replaces the calendar's Monday lead (`reel-highlights`); post one or the other, not both. Skip a community
post if the sidebar bans self-promotion. Do not post a community thread on a day you cannot stay in it.

## When to post

Times are the audience's local time. I did not measure your audience: the sources below are community and
vendor studies, and the guesses about who your audience is are mine.

| Platform | Best times | Avoid | Source |
|---|---|---|---|
| LinkedIn | Tue 11am-5pm, Wed 11am-4pm, Thu 11am and 1-5pm. Tech/software: Tue 10am-2pm, Wed 10am-4pm | Weekends | <https://sproutsocial.com/insights/best-times-to-post-on-linkedin/> |
| Instagram | Mon 2-4pm, Tue 1-7pm, Wed 12-9pm, Thu 12-2pm | Weekends | <https://sproutsocial.com/insights/best-times-to-post-on-instagram/> |
| TikTok | Tue 2-6pm, Wed 1-8pm, Thu 1-5pm; Mon and Fri 3-5pm | Saturday and Sunday (Sunday worst) | <https://sproutsocial.com/insights/best-times-to-post-on-tiktok/> |
| X | Tue-Thu 12-6pm | Saturday (worst) | <https://sproutsocial.com/insights/best-times-to-post-on-twitter/> |
| Facebook | Tue-Wed 12-8pm | Weekends | <https://sproutsocial.com/insights/best-times-to-post-on-facebook/> |
| Overall | Tue and Wed, 11am-6pm | Sunday (worst) | <https://sproutsocial.com/insights/best-times-to-post-on-social-media/> |

Sprout Social 2026: about 2 billion engagements across 307k profiles, 27 Nov 2025 to 27 Feb 2026.

- **Show HN.** Myriade's analysis of 157k+ Show HN posts since 2009 (a "breakout" is 30 votes): best hour 12:00 UTC
  (12.2% breakout; 8am EDT, 5am PDT), best day Sunday (11.75%):
  <https://www.myriade.ai/blogs/when-is-it-the-best-time-to-post-on-show-hn>. Syften's guide calls 9am-12pm ET a practical
  default and says to treat timing as planning for when you can be there, not a ranking trick:
  <https://syften.com/blog/hacker-news-marketing/>. `calendar.md` needs you in the thread 4 to 6 hours: pick a weekday
  at 12:00 UTC you can stay for, or the Sunday. Never the same day as a Reddit post.
- **Product Hunt.** Launches go live at 12:01am Pacific. Tue-Thu has the most traffic and the most competition;
  weekends are easier to rank on and see less traffic. These are community guides, not data:
  <https://www.scrolllaunch.com/blog/best-day-to-launch-on-product-hunt-2026>, <https://www.launchpact.io/tools/launch-date-picker>.
  Give it its own day, not a Show HN day.
- **Launch-week skeleton.** Tue: LinkedIn 11am ET, X 12-1pm ET, Instagram and TikTok 2-4pm ET (theme 1). Wed: theme 2,
  Show HN at 12:00 UTC. Thu: theme 4. Product Hunt on a later Tue-Thu.
- **Your clock.** Convert from the audience's time: for a US East audience add 5 hours to get UK time, except
  25 Oct to 1 Nov, when the UK is on GMT and the US still on EDT (add 4). For the UK and EU use the times as written.
  That most of the audience is in the US and the EU is an assumption: check the platforms' own insights after week one.

## Product Hunt gallery

Product Hunt recommends gallery images of **1270x760** and needs 2 or more before the gallery shows; video is YouTube
links only (checked live, `maxAge: 0`, 2026-10-24: <https://help.producthunt.com/en/articles/479557-how-to-post-a-product>;
the same size is on <https://www.producthunt.com/launch/preparing-for-launch>). None of the 42 is exactly that ratio:
1280x720 is the closest (16:9, against 1270x760's 1.67:1), so expect a little letterboxing or crop and check the
preview before you publish. Use the 1280x720 of themes 1, 2, 3, 5 and 4 in that order (five images, as
`launch/product-hunt.md` plans); keep the "Real app, real Claude" labels, and put the demo's YouTube link in the video slot.

## Link format

`https://edytlab.com/videos/<slug>?utm_source=<platform>&utm_medium=social&utm_campaign=launch&utm_content=<theme>`

| Platform | `utm_source` | | Platform | `utm_source` |
|---|---|---|---|---|
| X | `x` | | Instagram (bio link) | `instagram` |
| LinkedIn | `linkedin` | | TikTok (bio link) | `tiktok` |
| Facebook | `facebook` | | YouTube | `youtube` |
| Threads | `threads` | | Pinterest | `pinterest` |
| Bluesky | `bluesky` | | Product Hunt (Website field) | `producthunt` |

`utm_content` is the theme's slug (`describe-the-edit`, `dj-transition`, `club-intro`, `local-audio`, `versions`,
`open-source`). Leave the UTM off Show HN and Reddit links (a clean URL reads better there), and off GitHub links.
The site's analytics only count a campaign once the owner adds the GA4 measurement ID
(`NEXT_PUBLIC_GA_MEASUREMENT_ID`), and then only for visitors who accept the consent banner; until then the only signals
are GitHub downloads, stars and issues (`calendar.md`, "Measuring"). I did not check whether the site's anonymous
Vercel analytics breaks out UTM parameters.

## Don't

- No upvote, star or share asks, on any platform.
- Never post the same text twice: every caption in `captions.md` is written for its platform.
- Keep the "Real app, real Claude" and "Real app, recorded live" labels on every frame; do not crop them out.
- No stem separation, vocal isolation or mashup claims, and no version number, count or ranking beyond what is on
  the images (94 tools, six providers, MIT, unsigned installers).
- No "best" or "fastest". Say "unsigned" in any post that links a download.
- Do not stack two community posts on one day, and do not post a community thread you cannot stay in.
