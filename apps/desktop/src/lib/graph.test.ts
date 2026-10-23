import { describe, expect, it } from "vitest";
import { degrees, nearest, neighbours, radius } from "./graph";

describe("graph geometry", () => {
  it("counts the links of every note, isolated ones as 0", () => {
    expect(degrees(4, [[0, 1], [1, 2]])).toEqual([1, 2, 1, 0]);
  });

  it("makes busier notes bigger, but not without limit", () => {
    expect(radius(0)).toBeLessThan(radius(1));
    expect(radius(1)).toBeLessThan(radius(9));
    expect(radius(100)).toBeLessThan(20);
  });

  it("finds the dot under a point, the closest one when they overlap", () => {
    const pts = [{ x: 0, y: 0, r: 5 }, { x: 6, y: 0, r: 5 }, { x: 50, y: 50, r: 3 }];
    expect(nearest(pts, 1, 0)).toBe(0);
    expect(nearest(pts, 5, 0)).toBe(1);
    expect(nearest(pts, 20, 20)).toBe(-1);
    expect(nearest(pts, 50, 56)).toBe(-1);
    expect(nearest(pts, 50, 56, 4)).toBe(2);
  });

  it("lists the neighbours both ways", () => {
    const n = neighbours(3, [[0, 2]]);
    expect([...n[0]]).toEqual([2]);
    expect([...n[2]]).toEqual([0]);
    expect(n[1].size).toBe(0);
  });
});
