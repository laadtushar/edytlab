import { assert, onboard, sleep } from "./helpers.mjs";

export default [
  {
    id: "1-first-launch",
    area: "Onboarding & Settings",
    title: "A first launch welcomes me and asks for an AI provider before anything else",
    async run(ctx) {
      const { d } = ctx;
      await d.waitFor("[data-testid='settings']");
      await sleep(800);
      assert((await d.attr("[data-testid='settings']", "data-mode")) === "blocking", "welcome is blocking");
      await ctx.shot("The welcome dialog: six providers, a key field, a base URL and a model");
      await d.click("[data-testid='settings-provider-groq']");
      await sleep(400);
      await ctx.shot("Choosing Groq asks for a Groq key");
      await d.click("[data-testid='settings-provider-ollama']");
      await d.until(async () => (await d.count("[data-testid='settings']")) === 0, { label: "welcome to close" });
      assert((await d.invoke("get_active_provider")) === "ollama", "Ollama is active");
      await sleep(500);
      await ctx.shot("Choosing local Ollama needs no key, so the welcome closes onto the editor");
    },
  },
  {
    id: "1-settings-panel",
    area: "Onboarding & Settings",
    title: "From the gear I can see my provider's models and test that it can call tools",
    async run(ctx) {
      const { d } = ctx;
      await onboard(ctx);
      await d.click("[data-testid='open-settings-button']");
      await d.waitFor("[data-testid='settings']");
      assert((await d.attr("[data-testid='settings']", "data-mode")) === "panel", "panel mode");
      await d.waitFor("[data-testid='settings-model-hint']", { pred: (t) => /1 model/i.test(t) });
      await sleep(400);
      await ctx.shot("Settings as a panel: Ollama selected, one model listed from the local server");
      await ctx.script({ stream: [{ text: "", tool_calls: [{ name: "get_audio_info", arguments: {} }] }] });
      await d.click("[data-testid='settings-test-button']");
      const outcome = await d.until(async () => {
        for (const id of ["settings-test-ok", "settings-test-no-tools", "settings-test-error"]) {
          if (await d.count(`[data-testid='${id}']`)) return id;
        }
        return false;
      }, { timeout: 20000, label: "test result" });
      await ctx.shot(`The connection test reports: ${outcome}`);
      assert(outcome === "settings-test-ok", `test said ${outcome}: ${await d.text(`[data-testid='${outcome}']`)}`);
      for (const tab of ["project", "memory", "skills", "agents", "mcp", "plugins"]) {
        await d.click(`[data-testid='settings-tab-${tab}']`);
        await sleep(500);
        await ctx.shot(`Settings, ${tab} tab`);
      }
      await d.click("[data-testid='settings-close']");
      await d.until(async () => (await d.count("[data-testid='settings']")) === 0, { label: "panel to close" });
    },
  },
  {
    id: "1-provider-without-key",
    area: "Onboarding & Settings",
    title: "Switching to a provider I have no key for warns me rather than failing later",
    async run(ctx) {
      const { d } = ctx;
      await onboard(ctx);
      await d.click("[data-testid='open-settings-button']");
      await d.click("[data-testid='settings-provider-openai']");
      await d.waitFor("[data-testid='settings-save-error']", { timeout: 10000 });
      await ctx.shot("A warning: OpenAI has no key stored");
    },
  },
];
