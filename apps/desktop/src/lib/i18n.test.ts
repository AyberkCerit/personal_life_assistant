import { afterEach, describe, expect, it, vi } from "vitest";
import { errorText, lookup, sayAgain, setLang } from "./i18n";

// M6 translations (FR-I18N-001/002/007): a command's `code|detail|reason` said in the chosen language
describe("errorText", () => {
  afterEach(() => setLang("tr"));

  it("says a known code with its detail; the reason is only for copying", () => {
    setLang("tr");
    const e = errorText("read_only|notes/a.md|Access is denied. (os error 5)");
    expect(e.text).toContain("notes/a.md");
    expect(e.text).not.toContain("Access is denied");
    expect(e.details).toBe("Access is denied. (os error 5)");
  });

  it("prefers the area's own message", () => {
    setLang("tr");
    expect(errorText("missing||", "tree").text).toBe(lookup("tree.error.missing"));
    expect(errorText("missing||", "nowhere").text).toBe(lookup("error.missing")!.replace(" ({detail})", ""));
  });

  it("an unknown code or plain text is unexpected, and copied whole", () => {
    setLang("en");
    const e = errorText("llama-server exited during startup: exit code 1");
    expect(e.text).toBe(lookup("error.unexpected"));
    expect(e.details).toBe("llama-server exited during startup: exit code 1");
    expect(errorText("weird_code|x|y").text).toBe(lookup("error.unexpected"));
  });

  it("the language changes what it says, not what it copies", () => {
    setLang("tr");
    const tr = errorText("disk_full||No space left on device");
    setLang("en");
    const en = errorText("disk_full||No space left on device");
    expect(tr.text).not.toBe(en.text);
    expect(tr.details).toBe(en.details);
  });
});

describe("lookup", () => {
  it("falls back to English and says so when a key is missing", () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    setLang("tr");
    expect(lookup("error.no_such_key_anywhere")).toBeNull();
    expect(warn).toHaveBeenCalled();
    warn.mockRestore();
  });
});

describe("errorText detail", () => {
  it("names the folder when there is one, and reads well when there is none", () => {
    setLang("tr");
    expect(errorText("missing|C:/Notlar|os error 2").text).toContain("(C:/Notlar)");
    expect(errorText("missing||os error 2").text).not.toContain("()");
  });
});

describe("final review (M6 translations)", () => {
  it("knows an area's code that has no detail and no pipe", () => {
    setLang("tr");
    expect(errorText("not_found", "metrics").text).toBe(lookup("metrics.error.not_found"));
  });

  it("says an open message again in the new language", () => {
    setLang("tr");
    const tr = errorText("disk_full||x");
    setLang("en");
    expect(sayAgain(tr)).toEqual(errorText("disk_full||x"));
    expect(sayAgain("already said")).toBe("already said");
    setLang("tr");
  });
});
