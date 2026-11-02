import { describe, expect, it } from "vitest";
import { degrees, nearest, neighbours, overlaps, radius, shortLabel } from "./graph";

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

  it("tells label boxes that cover each other from ones that only touch", () => {
    expect(overlaps([0, 0, 10, 4], [5, 2, 10, 4])).toBe(true);
    expect(overlaps([0, 0, 10, 4], [10, 0, 10, 4])).toBe(false);
    expect(overlaps([0, 0, 10, 4], [0, 4, 10, 4])).toBe(false);
  });

  it("cuts long titles, Turkish letters intact", () => {
    expect(shortLabel("PLA")).toBe("PLA");
    const long = shortLabel("Çok Uzun Bir Başlığı Olan Ve Grafta Etiketi Taşabilecek Bir Not");
    expect([...long].length).toBe(28);
    expect(long.endsWith("…")).toBe(true);
    expect(long.startsWith("Çok Uzun Bir Başlığı")).toBe(true);
  });
});
