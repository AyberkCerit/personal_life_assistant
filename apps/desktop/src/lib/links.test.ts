import { describe, expect, it } from "vitest";
import { completionQuery, linkAt, linkRest, snippetParts } from "./links";

describe("links helpers", () => {
  it("finds the wikilink under the cursor", () => {
    const line = "Bugün [[Proje Planı|planı]] ve [[Günlük#Sabah]] okudum.";
    expect(linkAt(line, 10)).toBe("Proje Planı");
    expect(linkAt(line, 6)).toBe("Proje Planı"); // on the opening brackets
    expect(linkAt(line, 36)).toBe("Günlük#Sabah");
    expect(linkAt(line, 2)).toBeNull();
    expect(linkAt("resim ![[a.png]]", 10)).toBeNull(); // an embed is not a link to open
    expect(linkAt("[[]] boş", 1)).toBeNull();
    expect(linkAt("[[#Başlık]] aynı notta", 3)).toBeNull(); // a heading of this note: no note to open
  });

  it("knows when the user is typing a link name", () => {
    expect(completionQuery("Bugün [[Pro")).toEqual({ query: "Pro", offset: 8 });
    expect(completionQuery("[[")).toEqual({ query: "", offset: 2 });
    expect(completionQuery("[[Bitti]] sonra")).toBeNull();
    expect(completionQuery("tek [ köşeli")).toBeNull();
  });

  it("knows how much of an existing link a completion replaces", () => {
    // links final review M3: picking inside [[Pro|je]] must not leave "je]]" behind
    expect(linkRest("je]] sonra")).toEqual({ length: 2, closed: true });
    expect(linkRest("|takma]]")).toEqual({ length: 0, closed: true });
    expect(linkRest(" sonra yazı")).toEqual({ length: 0, closed: false });
    expect(linkRest("x [[başka]]")).toEqual({ length: 0, closed: false });
  });

  it("splits a snippet into plain and matched parts without any markup", () => {
    // Review Focus 5: the parts are rendered as text
    expect(snippetParts("…yarın \u0002Dişçi\u0003 <b>var</b>…")).toEqual([
      { text: "…yarın ", hit: false },
      { text: "Dişçi", hit: true },
      { text: " <b>var</b>…", hit: false },
    ]);
    expect(snippetParts("")).toEqual([]);
  });
});
