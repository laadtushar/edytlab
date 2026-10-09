import onboarding from "./onboarding.mjs";
import projects from "./projects.mjs";
import timeline from "./timeline.mjs";
import agent from "./agent.mjs";
import plan from "./plan.mjs";
import undo from "./undo.mjs";
import assistant from "./assistant.mjs";
import playback from "./playback.mjs";
import clips from "./clips.mjs";
import claude from "./claude.mjs";
import demos from "./demos.mjs";

export const stories = [...onboarding, ...projects, ...timeline, ...agent, ...plan, ...undo, ...assistant, ...playback, ...clips, ...claude, ...demos];
