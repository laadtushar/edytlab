import type { JSX } from "react";

import { Chat } from "./chat";
import { ChatGptPlan } from "./chatgpt-plan";
import { EffectsAndTemplates } from "./effects-and-templates";
import { ExportAndMemory } from "./export-and-memory";
import { PlaybackAndCompare } from "./playback-and-compare";
import { Providers } from "./providers";
import { SessionsAndTracks } from "./sessions-and-tracks";
import { SkillsAndProfiles } from "./skills-and-profiles";
import { Speech } from "./speech";

/** The prose of each user-guide page, by the slug in `lib/docs/user-guide-data.ts`. */
export const guideContent: Readonly<Record<string, () => JSX.Element>> = {
  "sessions-and-tracks": SessionsAndTracks,
  "effects-and-templates": EffectsAndTemplates,
  chat: Chat,
  "playback-and-compare": PlaybackAndCompare,
  speech: Speech,
  "export-and-memory": ExportAndMemory,
  "skills-and-profiles": SkillsAndProfiles,
  providers: Providers,
  "chatgpt-plan": ChatGptPlan,
};
