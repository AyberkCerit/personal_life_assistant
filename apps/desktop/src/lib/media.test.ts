import { describe, expect, it } from "vitest";
import { fold, frontmatterLines, ImageCache, imageEmbeds, isImageFile, mediaError } from "./media";

describe("media helpers", () => {
  it("finds the frontmatter block only when it closes", () => {
    // FR-EDT-006/007
    expect(frontmatterLines(["---", "tags: [a]", "---", "# Not"])).toBe(3);
    expect(frontmatterLines(["﻿---", "...", "metin"])).toBe(2);
    expect(frontmatterLines(["---", "kapanmıyor: evet"])).toBe(0);
    expect(frontmatterLines(["# Not", "---"])).toBe(0);
  });

  it("finds image embeds on a line, not links or code", () => {
    // FR-EDT-016
    expect(imageEmbeds("Bkz ![[şema.png]] ve ![[img/foto.JPG|300]]")).toEqual(["şema.png", "img/foto.JPG"]);
    expect(imageEmbeds("[[şema.png]] ![[Not]] `![[kod.png]]` ![[a.webp#x]]")).toEqual(["a.webp"]);
    expect(imageEmbeds("| ![[t.gif\\|100]] |")).toEqual(["t.gif"]);
  });

  it("takes image files only", () => {
    expect(isImageFile({ type: "image/png", name: "" })).toBe(true);
    expect(isImageFile({ type: "", name: "Foto.JPEG" })).toBe(true);
    expect(isImageFile({ type: "application/pdf", name: "a.pdf" })).toBe(false);
  });

  it("folds like the index (Turkish İ and I are i)", () => {
    expect(fold("Işık")).toBe("işık");
    expect(fold("İş")).toBe("iş");
    expect(fold("I.png") === fold("ı.png")).toBe(false);
  });

  it("reads save errors", () => {
    expect(mediaError("too_big|20|")).toEqual({ key: "image.error.too_big", reason: "" });
    expect(mediaError("not_image||")).toEqual({ key: "image.error.not_image", reason: "" });
    expect(mediaError("io||disk dolu")).toEqual({ key: "image.error.io", reason: "disk dolu" });
  });

  it("fetches each image once and forgets the missing ones", async () => {
    const asked: string[] = [];
    const cache = new ImageCache(async (t) => {
      asked.push(t);
      if (t.startsWith("yok")) throw "missing";
      return new ArrayBuffer(4);
    });
    const a = await cache.get("a.png");
    expect(a).toMatch(/^blob:/);
    expect(await cache.get("A.png")).toBe(a);
    expect(await cache.get("yok.png")).toBeNull();
    cache.forgetMissing(); // at once: the next preview asks again
    await cache.get("yok.png");
    expect(asked).toEqual(["a.png", "yok.png", "yok.png"]);
    cache.dispose();
  });
});
