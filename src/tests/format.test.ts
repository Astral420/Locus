import { describe, it, expect } from "vitest";
import { formatElapsed, dbfsToPercent, formatDbfs } from "../lib/format";

describe("formatElapsed", () => {
  it("never renders fractional seconds (regression: 00:9.00096642...)", () => {
    expect(formatElapsed(9.00096642)).toBe("00:09");
    expect(formatElapsed(0.0000664)).toBe("00:00");
    expect(formatElapsed(59.999)).toBe("00:59");
  });
  it("rolls over to hours", () => {
    expect(formatElapsed(3600)).toBe("01:00:00");
    expect(formatElapsed(3725.7)).toBe("01:02:05");
  });
  it("guards invalid input", () => {
    expect(formatElapsed(NaN)).toBe("00:00");
    expect(formatElapsed(-5)).toBe("00:00");
  });
});

describe("dBFS meter helpers", () => {
  it("maps -60..0 dBFS to 0..100%", () => {
    expect(dbfsToPercent(-60)).toBe(0);
    expect(dbfsToPercent(-30)).toBe(50);
    expect(dbfsToPercent(0)).toBe(100);
  });
  it("clamps and handles missing values", () => {
    expect(dbfsToPercent(-120)).toBe(0);
    expect(dbfsToPercent(6)).toBe(100);
    expect(dbfsToPercent(undefined)).toBe(0);
    expect(dbfsToPercent(-Infinity)).toBe(0);
  });
  it("labels silence and readings", () => {
    expect(formatDbfs(-90)).toBe("-inf dBFS");
    expect(formatDbfs(-12.4)).toBe("-12 dBFS");
    expect(formatDbfs(null)).toBe("-inf dBFS");
  });
});
