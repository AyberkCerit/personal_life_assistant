import { afterEach, describe, expect, it, vi } from "vitest";
import { createRawSnippet } from "svelte";
import { render } from "svelte/server";
import Bell from "@lucide/svelte/icons/bell";
import Icon from "./Icon.svelte";
import Button from "./Button.svelte";
import IconButton from "./IconButton.svelte";
import Badge from "./Badge.svelte";
import Checkbox from "./Checkbox.svelte";
import Logo from "./Logo.svelte";
import { BASE, motion } from "./motion";

const text = (s: string) => createRawSnippet(() => ({ render: () => `<span>${s}</span>` }));

describe("ui components", () => {
  it("hides decorative icons from screen readers and names meaningful ones", () => {
    expect(render(Icon, { props: { icon: Bell } }).body).toContain('aria-hidden="true"');
    const named = render(Icon, { props: { icon: Bell, label: "Hatırlatıcı" } }).body;
    expect(named).toContain('aria-label="Hatırlatıcı"');
    expect(named).not.toContain('aria-hidden="true"');
  });

  it("never submits a form by accident", () => {
    expect(render(Button, { props: { children: text("Kaydet") } }).body).toContain('type="button"');
    expect(render(Button, { props: { type: "submit", children: text("Kaydet") } }).body).toContain('type="submit"');
  });

  it("names icon-only buttons", () => {
    const body = render(IconButton, { props: { icon: Bell, label: "Sil" } }).body;
    expect(body).toContain('aria-label="Sil"');
    expect(body).toContain('title="Sil"');
  });

  it("marks AI with the sparkles icon and only there", () => {
    expect(render(Badge, { props: { kind: "ai", children: text("AI") } }).body).toMatch(/lucide-sparkles/);
    expect(render(Badge, { props: { kind: "neutral", children: text("3") } }).body).not.toMatch(/lucide-sparkles/);
  });

  it("keeps a real, focusable checkbox under the custom box", () => {
    const body = render(Checkbox, { props: { label: "Hatırlat" } }).body;
    expect(body).toContain('type="checkbox"');
    expect(body).not.toMatch(/display:\s*none/);
    expect(body).toContain("Hatırlat");
    expect(render(Checkbox, { props: { label: "Tamamlandı", hideLabel: true } }).body).toContain('aria-label="Tamamlandı"');
  });

  it("gives the logo an accessible name", () => {
    expect(render(Logo, { props: {} }).body).toContain('aria-label="PLA"');
  });
});

describe("motion", () => {
  afterEach(() => vi.unstubAllGlobals());

  it("plays at full length normally", () => {
    vi.stubGlobal("matchMedia", () => ({ matches: false }));
    expect(motion(BASE)).toBe(200);
  });

  it("does not animate when the user asked for reduced motion", () => {
    vi.stubGlobal("matchMedia", (q: string) => ({ matches: q.includes("reduce") }));
    expect(motion(BASE)).toBe(0);
  });

  it("plays when matchMedia is not available", () => {
    expect(motion(BASE)).toBe(200);
  });
});
