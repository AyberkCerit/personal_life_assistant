// Images and frontmatter in the editor (FR-EDT-006/007/016/017): which lines are frontmatter,
// which embeds are images, which files may be pasted, and what a failed save means.
import type { Key } from "./i18n";

const IMAGE = /\.(png|jpe?g|gif|webp)$/i;
const IMAGE_TYPES = ["image/png", "image/jpeg", "image/gif", "image/webp"];

/** Lines the frontmatter takes, fences included; 0 when the note has none (it must close). */
export function frontmatterLines(lines: string[]): number {
  const first = (lines[0] ?? "").replace(/^﻿/, "").trimEnd();
  if (first !== "---") return 0;
  for (let i = 1; i < lines.length; i++) {
    const l = lines[i].trimEnd();
    if (l === "---" || l === "...") return i + 1;
  }
  return 0;
}

/** The image targets of the `![[…]]` embeds on a line (code spans left out), size/heading cut. */
export function imageEmbeds(line: string): string[] {
  const out: string[] = [];
  const re = /!\[\[([^\]\n]+)\]\]/g;
  for (let m = re.exec(line); m; m = re.exec(line)) {
    const ticks = line.slice(0, m.index).split("`").length - 1;
    if (ticks % 2 === 1) continue; // inside inline code
    const target = m[1].replace(/\\\|/g, "|").split("|")[0].split("#")[0].trim();
    if (IMAGE.test(target)) out.push(target);
  }
  return out;
}

/** A pasted or dropped file PLA stores (the backend checks the bytes again). */
export function isImageFile(file: { type: string; name: string }): boolean {
  return IMAGE_TYPES.includes(file.type) || IMAGE.test(file.name);
}

/** Reads `<code>|<detail>|<reason>` from `save_image`. */
export function mediaError(err: string): { key: Key; reason: string } {
  const [code, , ...rest] = err.split("|");
  if (code === "not_image") return { key: "image.error.not_image", reason: "" };
  if (code === "too_big") return { key: "image.error.too_big", reason: "" };
  return { key: "image.error.io", reason: code === "io" ? rest.join("|") : err };
}

/**
 * Image previews by embed target: each image is fetched once and shown from an object URL; a
 * missing one is `null`. `dispose` lets every URL go when the editor closes.
 */
export class ImageCache {
  private urls = new Map<string, Promise<string | null>>();
  constructor(private fetch: (target: string) => Promise<ArrayBuffer>) {}

  get(target: string): Promise<string | null> {
    const key = target.toLocaleLowerCase("tr");
    let url = this.urls.get(key);
    if (!url) {
      url = this.fetch(target).then(
        (bytes) => URL.createObjectURL(new Blob([bytes])),
        () => null,
      );
      this.urls.set(key, url);
    }
    return url;
  }

  /** Forget the images that were missing (one may just have been saved). */
  forgetMissing(): void {
    for (const [key, url] of this.urls) void url.then((u) => u === null && this.urls.get(key) === url && this.urls.delete(key));
  }

  dispose(): void {
    for (const url of this.urls.values()) void url.then((u) => u && URL.revokeObjectURL(u));
    this.urls.clear();
  }
}
