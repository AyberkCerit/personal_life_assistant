import { describe, expect, it } from "vitest";
import { metricError } from "./metrics";
import { mockBackend } from "./mock";

describe("mock backend", () => {
  it("refuses an unusual value in the backend's own format", async () => {
    // architecture review M9: `out_of_range|<field>|<value>`, as pla_core::metrics writes it
    const input = { kind: "weight", date: "2026-10-01", value: 450, unit: null, exercise: null, sets: null, reps: null, confirmed: false };
    const err = await mockBackend("metric_log", { input }).catch((e) => String(e));
    expect(metricError(err as string)).toMatchObject({ key: "metrics.error.out_of_range", field: "value", value: 450 });
  });
});
