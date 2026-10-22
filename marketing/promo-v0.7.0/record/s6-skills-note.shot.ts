/**
 * S6. The Skills editor says bundled skills are updated (#574).
 *
 * Settings, Skills tab. Until a skill is chosen, the editor's empty pane says
 * "Skills that come with edytlab are updated when a release changes them,
 * unless you have edited them." (`SkillsEditor.tsx`, `skills-bundled-note`).
 * The list is the eight skills in `src-tauri/resources/skills/`, answered as
 * `list_skills` answers them (see `lib/skills.ts`). Then one is opened, to
 * show that they are ordinary editable files.
 */

import { expect, test } from "../../../apps/desktop/e2e/fixtures";
import { byArg, ok, projectWith, settingsTabs } from "../../../apps/desktop/e2e/backend";

import { LENGTH_SEC, stems, writeFixture } from "./lib/music";
import { writeNotes } from "./lib/notes";
import { bundledSkills, skillSummaries } from "./lib/skills";
import { SCALE, VIEWPORT, Stage } from "./lib/stage";
import { Studio, loadedTrack } from "./lib/studio";

test.use({ viewport: VIEWPORT, deviceScaleFactor: SCALE });

const NAME = "s6-skills-note";
const OPEN = "loudness-master";

test(NAME, async ({ app }) => {
  const page = app.page;
  const stage = await Stage.prepare(app);

  const s = stems();
  const studio = new Studio();
  studio.tracks = [
    loadedTrack(1, "Backing", writeFixture("Music/demo-song/backing.wav", s.backing), LENGTH_SEC),
    loadedTrack(2, "Vocal", writeFixture("Music/demo-song/vocal.wav", s.vocal), LENGTH_SEC),
  ];
  studio.commit("load", { agoSec: 140 });
  studio.commit("load", { agoSec: 125 });

  const skills = bundledSkills();
  await app.boot({
    ...projectWith(studio.tracks, studio.head),
    ...studio.answers(),
    ...settingsTabs(),
    list_skills: ok(skillSummaries()),
    read_skill: byArg("name", Object.fromEntries(skills.map((k) => [k.name, ok(k)]))),
  });
  await expect(page.getByTestId("timeline-lane")).toHaveCount(2);
  await app.settle();
  await page.evaluate(() => document.fonts.ready);

  await stage.start(NAME);
  await stage.wait(900);

  await stage.click(page.getByTestId("open-settings-button"));
  await expect(page.getByTestId("settings-card")).toBeVisible();
  await stage.wait(900);

  await stage.click(page.getByTestId("settings-tab-skills"));
  await expect(page.getByTestId("skills-bundled-note")).toBeVisible();
  await expect(page.getByTestId("skills-bundled-note")).toContainText("updated when a release changes them");
  stage.mark("bundled note shown");
  await stage.wait(900);
  await stage.hover(page.getByTestId("skills-bundled-note"), { moveMs: 800 });
  await stage.wait(2600);

  // One of them, opened.
  await stage.click(page.getByTestId(`skills-row-${OPEN}`));
  await expect(page.getByTestId("skills-name")).toHaveValue(OPEN);
  stage.mark("a skill opened");
  await stage.wait(2200);

  const done = await stage.finish();
  writeNotes(NAME, { ...done, skills: skills.map((k) => k.name), opened: OPEN });
});
