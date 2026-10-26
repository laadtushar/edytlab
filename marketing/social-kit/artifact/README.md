# Launch hub page

`launch-hub.mjs` builds the launch hub: every post's text, the images, the videos and a checklist, as one page for the Artifact tool.

```
node marketing/social-kit/artifact/launch-hub.mjs
```

It reads the markdown under `marketing/` (the launch, social-kit and social folders, `press-kit.md`) and `website/lib/videos.ts` and `demos.ts`. It writes `out/launch-hub/` (git-ignored):

| File | What |
|---|---|
| `index.html` | The page, a fragment with no doctype, html, head or body tags. |
| `files.json` | Published path to the absolute path of the file to publish beside the page (`linkedin/`, `posters/`). |
| `existing-files.txt` | The 42 theme images (`img/...`) the published page already holds. They are not copied. |
| `checklist-ids.txt` | The checklist item ids. |

`launch-hub.css` and `launch-hub.client.js` are inlined into the page. Every text block is copied from its markdown file unchanged; the cards marked "derived" (post 3 as a document, L6, the press quote) say what was changed. A copy edit is: edit the markdown, run the command, publish again. The script stops if a block it expects is missing.

Checklist ticks use the Artifact `db` capability (document `launch/checklist`, `{done: {<id>: true}}`) and fall back to localStorage. The page declares `{db: {}}`.
