import { describe, expect, it } from "vitest";
import { activityReporter } from "./activity";

describe("activityReporter", () => {
  it("reports the first event and then at most once per interval", () => {
    let now = 0;
    let sent = 0;
    const report = activityReporter(() => sent++, () => now, 30_000);
    report();
    expect(sent).toBe(1);
    now = 10_000;
    report();
    now = 29_999;
    report();
    expect(sent).toBe(1);
    now = 30_000;
    report();
    expect(sent).toBe(2);
  });
});
