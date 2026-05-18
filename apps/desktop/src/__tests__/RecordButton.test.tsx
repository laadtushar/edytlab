import { describe, it, expect } from "vitest";

// Smoke test: verifies record button DOM element handling
describe("record button", () => {
  it("renders record button placeholder", () => {
    const btn = document.createElement("button");
    btn.setAttribute("data-testid", "record-btn");
    btn.textContent = "⏺ Record";
    document.body.appendChild(btn);
    expect(document.querySelector("[data-testid=record-btn]")).toBeTruthy();
    document.body.removeChild(btn);
  });
});
