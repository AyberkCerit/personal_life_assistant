// PLA SRS .docx üretici. Çalıştırma: node build.js  ->  ../../SRS.docx
const fs = require("fs");
const path = require("path");
const {
  Document, Packer, Paragraph, TextRun, HeadingLevel, AlignmentType, Table, TableRow, TableCell,
  WidthType, ShadingType, BorderStyle, ImageRun, PageBreak, Header, Footer, PageNumber,
  TableOfContents, LevelFormat, PageOrientation, SectionType, TableLayoutType, VerticalAlign,
} = require("docx");

const INK = "2D3142", MUTED = "4F5D75", SOFT = "7A8399", ACCENT = "EB6C36", RULE = "BFC0C0", HEAD_FILL = "E8E9EE", ZEBRA = "F7F7F9";
const FONT = "Calibri", MONO = "Consolas";
const PORTRAIT_W = 9072;   // A4, 2.5 cm kenar: 16 cm
const LANDSCAPE_W = 13994; // A4 yatay: 24.7 cm
const DIAG = path.join(__dirname, "..", "diagrams");

const meta = require("./content/meta");
const parts = ["c1_giris", "c2_genel", "c3_ozellikler_a", "c3_ozellikler_b", "c3_ozellikler_c", "c4_arayuzler", "c5_nfr", "c6_ekler"]
  .map((n) => require("./content/" + n));

// ---------------------------------------------------------------- inline markup: **kalın**, `kod`, _italik_
function runs(text, base = {}) {
  const out = [];
  const re = /(\*\*[^*]+\*\*|`[^`]+`|\^\^[^^]+\^\^)/g;
  let last = 0, m;
  const s = String(text);
  while ((m = re.exec(s))) {
    if (m.index > last) out.push(new TextRun({ text: s.slice(last, m.index), ...base }));
    const tok = m[0];
    if (tok.startsWith("**")) out.push(...runs(tok.slice(2, -2), { ...base, bold: true }));
    else if (tok.startsWith("`")) out.push(new TextRun({ text: tok.slice(1, -1), ...base, font: MONO, size: base.size ? base.size - 1 : 19, color: base.color || INK }));
    else out.push(...runs(tok.slice(2, -2), { ...base, italics: true }));
    last = m.index + tok.length;
  }
  if (last < s.length) out.push(new TextRun({ text: s.slice(last), ...base }));
  return out;
}

const P = (text, opts = {}) => new Paragraph({ children: runs(text, opts.run || {}), spacing: { after: 120, line: 276 }, ...opts.para });

const border = { style: BorderStyle.SINGLE, size: 4, color: RULE };
const borders = { top: border, bottom: border, left: border, right: border };

function cell(content, width, { head = false, mono = false, size = 18, fill, align, bold = false } = {}) {
  const paras = String(content).split("\n").map((line) => new Paragraph({
    children: runs(line, { size, font: mono ? MONO : FONT, bold: head || bold, color: head ? INK : INK }),
    alignment: align || AlignmentType.LEFT, spacing: { after: 20, before: 20 },
  }));
  return new TableCell({
    children: paras, width: { size: width, type: WidthType.DXA }, borders,
    shading: head ? { fill: HEAD_FILL, type: ShadingType.CLEAR, color: "auto" } : fill ? { fill, type: ShadingType.CLEAR, color: "auto" } : undefined,
    margins: { top: 50, bottom: 50, left: 70, right: 70 }, verticalAlign: VerticalAlign.TOP,
  });
}

function table(head, rows, widthsPct, total, opts = {}) {
  const widths = widthsPct.map((p) => Math.round((total * p) / 100));
  widths[widths.length - 1] = total - widths.slice(0, -1).reduce((a, b) => a + b, 0);
  const mono = opts.mono || [];
  const size = opts.size || 18;
  const trs = [];
  if (head) trs.push(new TableRow({ tableHeader: true, children: head.map((h, i) => cell(h, widths[i], { head: true, size })) }));
  rows.forEach((r, ri) => trs.push(new TableRow({
    cantSplit: true,
    children: r.map((c, i) => cell(c, widths[i], { mono: mono.includes(i), size, fill: ri % 2 ? ZEBRA : undefined, bold: (opts.boldCol === i) })),
  })));
  return new Table({ width: { size: total, type: WidthType.DXA }, columnWidths: widths, rows: trs, layout: TableLayoutType.FIXED });
}

const EARS = { U: "Her zaman", E: "Olay", S: "Durum", W: "İstenmeyen", O: "Opsiyonel", C: "Karmaşık" };
const PRI = { Y: "Yüksek", O: "Orta", D: "Düşük" };

let figNo = 0, tblNo = 0;
function caption(text, kind) {
  return new Paragraph({
    children: [new TextRun({ text, size: 17, italics: true, color: MUTED })],
    alignment: AlignmentType.CENTER, spacing: { before: 60, after: 200 },
  });
}

function render(block, W) {
  const out = [];
  switch (block.t) {
    case "h1": out.push(new Paragraph({ heading: HeadingLevel.HEADING_1, children: [new TextRun(block.x)], pageBreakBefore: block.pb !== false })); break;
    case "h2": out.push(new Paragraph({ heading: HeadingLevel.HEADING_2, children: [new TextRun(block.x)], pageBreakBefore: !!block.pb })); break;
    case "h3": out.push(new Paragraph({ heading: HeadingLevel.HEADING_3, children: [new TextRun(block.x)] })); break;
    case "p": out.push(P(block.x)); break;
    case "bul": block.items.forEach((it) => out.push(new Paragraph({ numbering: { reference: "bul", level: block.lvl || 0 }, children: runs(it), spacing: { after: 60, line: 264 } }))); break;
    case "num": block.items.forEach((it) => out.push(new Paragraph({ numbering: { reference: block.ref || "num", level: 0 }, children: runs(it), spacing: { after: 60, line: 264 } }))); break;
    case "note": out.push(new Paragraph({
      children: runs(block.x, { size: 19, color: INK }), spacing: { before: 80, after: 160 },
      shading: { fill: "FDF1EB", type: ShadingType.CLEAR, color: "auto" },
      border: { left: { style: BorderStyle.SINGLE, size: 18, color: ACCENT, space: 6 } }, indent: { left: 120 },
    })); break;
    case "tbl": {
      if (block.cap) tblNo++;
      if (block.cap) out.push(new Paragraph({ children: runs(`Tablo ${tblNo} — ${block.cap}`, { size: 17, bold: true, color: MUTED }), spacing: { before: 120, after: 60 }, keepNext: true }));
      out.push(table(block.head, block.rows, block.w, W, { mono: block.mono, size: block.size, boldCol: block.boldCol }));
      out.push(new Paragraph({ children: [], spacing: { after: 120 } }));
      break;
    }
    case "reqs": {
      tblNo++;
      out.push(new Paragraph({ children: [new TextRun({ text: `Tablo ${tblNo} — ${block.cap || "Gereksinimler"}`, size: 17, bold: true, color: MUTED })], spacing: { before: 120, after: 60 }, keepNext: true }));
      const rows = block.rows.map((r) => [r[0], EARS[r[1]] || r[1], r[2], PRI[r[3]] || r[3], r[4], r[5]]);
      out.push(table(["Kimlik", "EARS", "Gereksinim", "Öncelik", "Doğr.", "Kaynak"], rows, [15, 13, 47, 9, 6, 10], W, { mono: [0], size: 17 }));
      out.push(new Paragraph({ children: [], spacing: { after: 120 } }));
      break;
    }
    case "sr": {
      tblNo++;
      out.push(new Paragraph({ children: [new TextRun({ text: `Tablo ${tblNo} — ${block.cap || "Uyarıcı ve tepki dizileri"}`, size: 17, bold: true, color: MUTED })], spacing: { before: 120, after: 60 }, keepNext: true }));
      out.push(table(["#", "Uyarıcı (kullanıcı eylemi / olay)", "Sistem tepkisi"], block.rows.map((r, i) => [String(i + 1), r[0], r[1]]), [5, 40, 55], W, { size: 18 }));
      out.push(new Paragraph({ children: [], spacing: { after: 120 } }));
      break;
    }
    case "ipo": {
      tblNo++;
      out.push(new Paragraph({ children: [new TextRun({ text: `Tablo ${tblNo} — ${block.cap || "Girdi, işleme ve çıktılar"}`, size: 17, bold: true, color: MUTED })], spacing: { before: 120, after: 60 }, keepNext: true }));
      out.push(table(["Girdiler", "Kurallar ve veri işleme adımları", "Çıktılar"], [[block.i.join("\n"), block.p.join("\n"), block.o.join("\n")]], [28, 44, 28], W, { size: 18 }));
      out.push(new Paragraph({ children: [], spacing: { after: 120 } }));
      break;
    }
    case "img": {
      figNo++;
      const file = path.join(DIAG, block.f);
      const buf = fs.readFileSync(file);
      const pxW = buf.readUInt32BE(16), pxH = buf.readUInt32BE(20);
      const wIn = block.w || (W === PORTRAIT_W ? 6.3 : 9.2);
      let wPx = Math.round(wIn * 96), hPx = Math.round((wPx * pxH) / pxW);
      const maxH = (W === PORTRAIT_W ? 8.6 : 4.9) * 96;
      if (hPx > maxH) { hPx = Math.round(maxH); wPx = Math.round((hPx * pxW) / pxH); }
      out.push(new Paragraph({ children: [new ImageRun({ type: "png", data: buf, transformation: { width: wPx, height: hPx }, altText: { title: block.cap, description: block.cap, name: block.f } })], alignment: AlignmentType.CENTER, spacing: { before: 120 }, keepNext: true }));
      out.push(caption(`Şekil ${figNo} — ${block.cap}`));
      break;
    }
    case "code": {
      const lines = block.x.split("\n");
      lines.forEach((line, i) => out.push(new Paragraph({
        children: [new TextRun({ text: line.length ? line : " ", font: MONO, size: 16, color: INK })],
        shading: { fill: "F4F4F6", type: ShadingType.CLEAR, color: "auto" },
        spacing: { before: i === 0 ? 80 : 0, after: i === lines.length - 1 ? 160 : 0, line: 240 },
        indent: { left: 120, right: 120 }, keepLines: true, keepNext: i < lines.length - 1,
      })));
      break;
    }
    case "pb": out.push(new Paragraph({ children: [new PageBreak()] })); break;
    case "sig": {
      block.rows.forEach(() => {});
      out.push(table(["Rol", "Ad Soyad", "Tarih", "İmza"], block.rows.map((r) => [r[0], r[1], "", ""]), [30, 30, 18, 22], W, { size: 18 }));
      out.push(new Paragraph({ children: [], spacing: { after: 120 } }));
      break;
    }
    default: throw new Error("bilinmeyen blok: " + block.t);
  }
  return out;
}

// ---------------------------------------------------------------- kapak
function cover() {
  const c = [];
  c.push(new Paragraph({ children: [], spacing: { before: 2200 } }));
  c.push(new Paragraph({ children: [new TextRun({ text: "YAZILIM GEREKSİNİMLERİ BELİRTİMİ", size: 22, color: ACCENT, bold: true, characterSpacing: 60 })], spacing: { after: 200 } }));
  c.push(new Paragraph({ children: [new TextRun({ text: meta.title, size: 56, bold: true, color: INK })], spacing: { after: 120 } }));
  c.push(new Paragraph({ children: [new TextRun({ text: meta.subtitle, size: 30, color: MUTED })], spacing: { after: 600 },
    border: { bottom: { style: BorderStyle.SINGLE, size: 8, color: ACCENT, space: 12 } } }));
  meta.coverRows.forEach(([k, v]) => c.push(new Paragraph({ children: [new TextRun({ text: k + ": ", bold: true, size: 22, color: INK }), new TextRun({ text: v, size: 22, color: INK })], spacing: { after: 80 } })));
  c.push(new Paragraph({ children: [], spacing: { before: 1800 } }));
  c.push(new Paragraph({ children: runs(meta.coverNote, { size: 18, color: SOFT, italics: true }) }));
  c.push(new Paragraph({ children: [new PageBreak()] }));
  // Sürüm geçmişi + içindekiler
  c.push(new Paragraph({ children: [new TextRun({ text: "Sürüm Geçmişi", bold: true, size: 28, color: INK })], spacing: { after: 160 } }));
  c.push(table(["Sürüm", "Tarih", "Yazan", "Açıklama"], meta.history, [12, 16, 22, 50], PORTRAIT_W, { size: 18 }));
  c.push(new Paragraph({ children: [], spacing: { after: 360 } }));
  c.push(new Paragraph({ children: [new TextRun({ text: "İçindekiler", bold: true, size: 28, color: INK })], spacing: { after: 160 } }));
  c.push(new TableOfContents("İçindekiler", { hyperlink: true, headingStyleRange: "1-2" }));
  return c;
}

// ---------------------------------------------------------------- belge
const header = new Header({ children: [new Paragraph({ alignment: AlignmentType.RIGHT, children: [new TextRun({ text: meta.headerText, size: 16, color: SOFT })] })] });
const footer = new Footer({ children: [new Paragraph({ alignment: AlignmentType.CENTER, children: [
  new TextRun({ text: "Sayfa ", size: 16, color: SOFT }), new TextRun({ children: [PageNumber.CURRENT], size: 16, color: SOFT }),
  new TextRun({ text: " / ", size: 16, color: SOFT }), new TextRun({ children: [PageNumber.TOTAL_PAGES], size: 16, color: SOFT })] })] });

const margin = { top: 1417, bottom: 1417, left: 1417, right: 1417 };
const portrait = { page: { size: { width: 11906, height: 16838 }, margin } };
const landscape = { page: { size: { width: 11906, height: 16838, orientation: PageOrientation.LANDSCAPE }, margin } };

// bölümleri yönlendirme değişimlerine göre grupla
const sections = [];
let cur = { props: portrait, W: PORTRAIT_W, children: cover(), first: true };
for (const part of parts) {
  for (const block of part) {
    if (block.t === "section") {
      sections.push(cur);
      const land = !!block.landscape;
      cur = { props: land ? landscape : portrait, W: land ? LANDSCAPE_W : PORTRAIT_W, children: [] };
      continue;
    }
    cur.children.push(...render(block, cur.W));
  }
}
sections.push(cur);

const doc = new Document({
  creator: meta.author, title: meta.title + " — SRS", description: meta.subtitle,
  styles: {
    default: { document: { run: { font: FONT, size: 21, color: INK } } },
    paragraphStyles: [
      { id: "Heading1", name: "Heading 1", basedOn: "Normal", next: "Normal", quickFormat: true,
        run: { size: 32, bold: true, font: FONT, color: INK }, paragraph: { spacing: { before: 240, after: 200 }, outlineLevel: 0,
        border: { bottom: { style: BorderStyle.SINGLE, size: 6, color: ACCENT, space: 4 } } } },
      { id: "Heading2", name: "Heading 2", basedOn: "Normal", next: "Normal", quickFormat: true,
        run: { size: 26, bold: true, font: FONT, color: INK }, paragraph: { spacing: { before: 300, after: 120 }, outlineLevel: 1, keepNext: true } },
      { id: "Heading3", name: "Heading 3", basedOn: "Normal", next: "Normal", quickFormat: true,
        run: { size: 22, bold: true, font: FONT, color: MUTED }, paragraph: { spacing: { before: 200, after: 100 }, outlineLevel: 2, keepNext: true } },
    ],
  },
  numbering: { config: [
    { reference: "bul", levels: [
      { level: 0, format: LevelFormat.BULLET, text: "•", alignment: AlignmentType.LEFT, style: { paragraph: { indent: { left: 540, hanging: 270 } } } },
      { level: 1, format: LevelFormat.BULLET, text: "–", alignment: AlignmentType.LEFT, style: { paragraph: { indent: { left: 1000, hanging: 270 } } } }] },
    { reference: "num", levels: [{ level: 0, format: LevelFormat.DECIMAL, text: "%1.", alignment: AlignmentType.LEFT, style: { paragraph: { indent: { left: 540, hanging: 320 } } } }] },
  ] },
  sections: sections.map((s, i) => ({
    properties: { ...s.props, ...(i > 0 ? { type: SectionType.NEXT_PAGE } : {}) },
    headers: { default: header }, footers: { default: footer },
    children: s.children,
  })),
});

const out = path.join(__dirname, "..", "..", "SRS.docx");
Packer.toBuffer(doc).then((buf) => { fs.writeFileSync(out, buf); console.log("yazıldı:", out, `(${figNo} şekil, ${tblNo} tablo)`); });
