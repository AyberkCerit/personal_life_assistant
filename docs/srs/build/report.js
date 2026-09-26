// PLA durum değerlendirmesi, risk analizi ve yol haritası (.docx). SRS'ten bağımsızdır.
// Çalıştırma: node report.js  ->  ../../PLA_Degerlendirme_Risk_Yol_Haritasi.docx
const fs = require("fs");
const path = require("path");
const {
  Document, Packer, Paragraph, TextRun, HeadingLevel, AlignmentType, Table, TableRow, TableCell,
  WidthType, ShadingType, BorderStyle, Header, Footer, PageNumber, LevelFormat, TableLayoutType, VerticalAlign,
} = require("docx");

const INK = "2D3142", MUTED = "4F5D75", SOFT = "7A8399", ACCENT = "EB6C36", RULE = "BFC0C0", HEAD = "E8E9EE";
const RED = "F6C9C4", ORANGE = "FBE3C8", GREEN = "DDEFD9";
const W = 9072, FONT = "Calibri", MONO = "Consolas";

function runs(text, base = {}) {
  const out = [], re = /(\*\*[^*]+\*\*|`[^`]+`)/g, s = String(text);
  let last = 0, m;
  while ((m = re.exec(s))) {
    if (m.index > last) out.push(new TextRun({ text: s.slice(last, m.index), ...base }));
    const t = m[0];
    if (t.startsWith("**")) out.push(...runs(t.slice(2, -2), { ...base, bold: true }));
    else out.push(new TextRun({ text: t.slice(1, -1), ...base, font: MONO, size: (base.size || 21) - 2 }));
    last = m.index + t.length;
  }
  if (last < s.length) out.push(new TextRun({ text: s.slice(last), ...base }));
  return out;
}
const P = (t) => new Paragraph({ children: runs(t), spacing: { after: 120, line: 276 } });
const H1 = (t) => new Paragraph({ heading: HeadingLevel.HEADING_1, children: [new TextRun(t)] });
const H2 = (t) => new Paragraph({ heading: HeadingLevel.HEADING_2, children: [new TextRun(t)] });
const B = (items, ref = "bul") => items.map((t) => new Paragraph({ numbering: { reference: ref, level: 0 }, children: runs(t), spacing: { after: 60, line: 264 } }));
const border = { style: BorderStyle.SINGLE, size: 4, color: RULE };
const borders = { top: border, bottom: border, left: border, right: border };
function cell(text, w, { head = false, fill, size = 18, align } = {}) {
  return new TableCell({
    width: { size: w, type: WidthType.DXA }, borders, verticalAlign: VerticalAlign.CENTER,
    shading: head ? { fill: HEAD, type: ShadingType.CLEAR, color: "auto" } : fill ? { fill, type: ShadingType.CLEAR, color: "auto" } : undefined,
    margins: { top: 50, bottom: 50, left: 80, right: 80 },
    children: String(text).split("\n").map((l) => new Paragraph({ children: runs(l, { size, bold: head }), alignment: align, spacing: { after: 20 } })),
  });
}
function table(head, rows, pct, fills = null, size = 18) {
  const ws = pct.map((p) => Math.round((W * p) / 100));
  ws[ws.length - 1] = W - ws.slice(0, -1).reduce((a, b) => a + b, 0);
  const trs = [new TableRow({ tableHeader: true, children: head.map((h, i) => cell(h, ws[i], { head: true, size })) })];
  rows.forEach((r, ri) => trs.push(new TableRow({ cantSplit: true, children: r.map((c, i) => cell(c, ws[i], { size, fill: fills ? fills(ri, i, r) : undefined })) })));
  return [new Table({ width: { size: W, type: WidthType.DXA }, columnWidths: ws, rows: trs, layout: TableLayoutType.FIXED }), new Paragraph({ children: [], spacing: { after: 120 } })];
}
const cap = (t) => new Paragraph({ children: [new TextRun({ text: t, size: 17, bold: true, color: MUTED })], spacing: { before: 120, after: 60 }, keepNext: true });

// ---------------------------------------------------------------- risk verisi
const risks = [
  ["R1", "Küçük yerel modelin Türkçe çıkarım doğruluğu hedefin (≥ %85) altında kalır.", 3, 3, "Denemede alan doğruluğu < %75", "Model fizibilite denemesini ilk iş yap (F1); iki adımlı çıkarım; 3–4B model; gerekirse fine-tuning'i (M7) öne çek veya kapsamı yalnız görev çıkarımına daralt."],
  ["R2", "Proje yeniden durur (okul yükü, motivasyon, uzun süre görünür sonuç olmaması). Nisan 2026'da başlayan proje 5,5 ay beklemişti.", 2, 3, "İki hafta boyunca çalışan kod commit'i yok", "2–3 haftalık kilometre taşları; her birinin sonunda çalışan demo; kendi günlük kullanımına erken sokmak; GitHub'da görünür ilerleme."],
  ["R3", "Belgeleme felci ve kapsam şişmesi: 267 gereksinim, 40 bekleyen karar; belgeler kodun önüne geçer.", 3, 2, "Karar turu 2 günü aşar; SRS yeniden açılır", "SRS'i v1.0 olarak dondur; değişiklikleri kodla birlikte küçük kayıtlarla (ADR) yap; kesme çizgisi belirle."],
  ["R4", "Kendi dosya yazma kodunda veri kaybı (Markdown bozma, üzerine yazma, yanlış yeniden adlandırma).", 2, 3, "Testte bozuk/yarım dosya", "Atomik yazma ve dış değişiklik testlerini M0'da yaz; gerçek vault yerine kopyasıyla dogfooding; basit yedeği erken ekle."],
  ["R5", "Varsayılan vault OneDrive'a düşer (bu makinede Belgeler = OneDrive\\Belgeler); senkron, atomik yeniden adlandırma ve `.pla/backup` kopyalarıyla çakışır.", 2, 3, "OneDrive \"çakışan kopya\" dosyaları", "Varsayılanı `%USERPROFILE%\\PLA Vault` yap veya OneDrive algılanınca uyar; senkron klasörde erken test."],
  ["R6", "Aynı anda çok yeni teknoloji (Rust, Tauri 2, Svelte 5, llama.cpp, sqlite-vec) — öğrenme eğrisi hızı düşürür.", 3, 2, "Basit bir özellik 1 haftadan uzun sürer", "Yürüyen iskelet; resmi şablonlar; her kilometre taşında tek yeni teknoloji; Claude Code + Superpowers ile TDD."],
  ["R7", "CPU'da gecikme ve RAM hedefleri tutmaz (8 GB makinede 2 GB model + WebView2 + tarayıcı).", 2, 2, "Denemede > 10 sn/not veya > 2 GB", "Ölçümleri 4 iş parçacığı ve GPU kapalıyla yap (geliştirme makinesi hedef donanımdan çok güçlü); daha küçük kuantizasyon."],
  ["R8", "Bulanık blok eşleştirme hataları: yinelenen veya yanlış güncellenen görevler.", 2, 2, "Kendi kullanımında yinelenen görev", "Basit başla (konum + satır benzerliği); kapsamlı birim testleri; İnceleme kutusu ve geri al."],
  ["R9", "Otomatik ekleme gürültüsü kullanıcının güvenini sarsar.", 2, 2, "Haftada > 3 gereksiz görev", "Güven eşiği; V2'deki \"onay modu\"nu ayar olarak öne çekmeye hazır ol."],
  ["R10", "Windows arka plan davranışı (tepsi, uyku/uyanma, bildirim izinleri) beklendiği gibi çalışmaz.", 2, 2, "Kaçırılan hatırlatıcı", "M2'de küçük bir prototiple erken doğrula."],
  ["R11", "Bağımlılık olgunluğu: sqlite-vec 1.0 öncesi, llama-server API değişimleri.", 2, 1, "Sürüm yükseltmede kırılma", "Sürüm sabitleme; ince adaptör katmanı."],
  ["R12", "İmzasız kurulum paketi ve sidecar .exe, SmartScreen/antivirüs tarafından engellenir.", 2, 1, "İlk kurulum testinde uyarı", "Erken paketleme testi; imzalama (TBD-07)."],
  ["R13", "Model lisansı ticari kullanımı kısıtlar (ör. Qwen2.5-3B).", 1, 2, "Seçilen modelin lisansı", "Apache-2.0/MIT modelleri tercih et; lisansı model kataloğunda sakla."],
];
const level = (s) => (s >= 6 ? ["Yüksek", RED] : s >= 3 ? ["Orta", ORANGE] : ["Düşük", GREEN]);
const riskRows = risks.map(([id, r, o, e, warn, act]) => [id, r, String(o), String(e), `${o * e} — ${level(o * e)[0]}`, warn, act]);

// risk matrisi (olasılık satır, etki sütun)
const grid = {};
for (const [id, , o, e] of risks) (grid[`${o}-${e}`] = grid[`${o}-${e}`] || []).push(id);
const matrixRows = [3, 2, 1].map((o) => [["Olasılık " + ["", "düşük (1)", "orta (2)", "yüksek (3)"][o]], ...[1, 2, 3].map((e) => (grid[`${o}-${e}`] || []).join(", ") || "—")].flat());

// ---------------------------------------------------------------- içerik
const children = [
  new Paragraph({ children: [new TextRun({ text: "PLA · DURUM DEĞERLENDİRMESİ", size: 20, bold: true, color: ACCENT, characterSpacing: 40 })], spacing: { after: 120 } }),
  new Paragraph({ children: [new TextRun({ text: "Gidişat, Risk Analizi ve Yol Haritası", size: 44, bold: true, color: INK })], spacing: { after: 80 } }),
  new Paragraph({ children: runs("27 Eylül 2026 · Dayanak: PRD v0.3 ve SRS v1.0 inceleme taslağı · Hazırlayan: Ayberk Cerit (Claude ile)", { size: 19, color: MUTED }), spacing: { after: 120 },
    border: { bottom: { style: BorderStyle.SINGLE, size: 8, color: ACCENT, space: 8 } } }),
  new Paragraph({ children: runs("Bu belge SRS'i değiştirmez; yalnız değerlendirme ve önerileri içerir. Öneriler kabul edilirse ilgili değişiklikler PRD v0.4 ve SRS v1.1'e işlenir.", { size: 18, italics: true, color: SOFT }), spacing: { after: 240 } }),

  H1("1. Yönetici Özeti"),
  ...B([
    "**Temel sağlam.** Kapsam, mimari ilkeler ve belgeler tutarlı. Üç gözden geçirme turunda beş kritik veri güvenliği açığı, daha kod yazılmadan kapatıldı.",
    "**Denge bozuluyor.** 66 sayfa SRS, 267 gereksinim, 40 bekleyen karar var ama henüz hiç kod yazılmadı. Daha fazla belge artık belirsizliği azaltmıyor.",
    "**En riskli varsayım test edilmedi.** Ürünün değeri, küçük yerel modelin Türkçe notlardan doğru çıkarım yapmasına bağlı. Bu varsayım ilk kilometre taşlarında değil, M1–M2'de sınanıyor.",
    "**Öncelikler ayırt edici değil.** Gereksinimlerin %79'u \"Yüksek\". Zaman daraldığında neyin bırakılacağı belli değil.",
    "**Önerilen sıra:** 1–2 günlük karar turu → 1 haftalık model fizibilite denemesi → 2 haftalık yürüyen iskelet → 3 haftalık dikey dilim (not → görev) ve demo.",
  ]),

  H1("2. Gidişat Çıkarımları"),
  cap("Tablo 1 — Durum göstergeleri"),
  ...table(["Gösterge", "Durum", "Yorum"], [
    ["Belgeler", "PRD v0.3, SRS v1.0 taslak (66 s.), 9 diyagram", "Yeterli; kodlamaya başlamak için eksik yok"],
    ["Kod", "0 satır", "Tüm commit'ler belge"],
    ["Açık kararlar", "40 inisiyatif kararı + 9 TBD", "Hiçbiri kodlamayı engellemiyor"],
    ["Öncelik dağılımı", "212 Yüksek / 53 Orta / 2 Düşük", "Kesme çizgisi yok"],
    ["Depo", "2 commit push bekliyor; README eski C++/Qt planını anlatıyor", "Portföy okuyucusunu yanıltır"],
    ["Proje geçmişi", "Nisan 2026 başlangıç → 5,5 ay ara → Eylül yeniden başlangıç", "Yeniden durma riski gerçek"],
  ], [20, 42, 38]),
  ...B([
    "**Ç1 — Belge yatırımı karşılığını verdi.** Vault başına veritabanı, hash tabanlı blok kimliği, göreli tarih referansı, çıkarım döngüsü ve yedek dışı kalan veri gibi sorunlar kodda bulunsaydı düzeltmesi çok daha pahalı olurdu.",
    "**Ç2 — Belgeleme doygunluğa ulaştı.** Kalan belirsizlikler (model kalitesi, bulanık eşleştirme, RAM, kullanıcı deneyimi) ancak kodla ve ölçümle çözülür.",
    "**Ç3 — Risk sırası ters.** M0 editörle başlıyor. Ürünün asıl değeri ve en büyük belirsizliği ise M1–M2'deki çıkarımda. Önce en riskli varsayım doğrulanmalı.",
    "**Ç4 — \"Yüksek\" önceliğin anlamı kalmadı.** 267 gereksinimin 212'si Yüksek. Tek kişilik bir projede bu, her şeyin eşit derecede zorunlu olduğu anlamına gelir ve planlamayı zorlaştırır.",
    "**Ç5 — Teknoloji yükü yüksek.** Rust, Tauri 2, Svelte 5, llama.cpp ve sqlite-vec aynı anda öğreniliyor. Geçmişte duraklamış bir projede bu, en olası yavaşlama nedenidir.",
    "**Ç6 — Varsayılan vault konumu senkron klasörüne düşüyor.** Bu makinede Belgeler = `OneDrive\\Belgeler`. SRS bulut senkron klasörünü destekleniyor sayıyor ama bu test edilmedi. Test edilmeden varsayılan konum olarak bırakılmamalı.",
    "**Ç7 — Portföy değeri demoyla başlar.** AI/ML stajı hedefi açısından belgeler ikincil kalır. Değer, çalışan bir dikey dilim ve ölçülmüş model sonuçlarıyla oluşur.",
  ]),

  H1("3. Risk Analizi"),
  P("Olasılık (O) ve etki (E) 1–3 arasında puanlandı; **skor = O × E**. Skoru 6 ve üzeri olanlar yüksek, 3–4 olanlar orta, 1–2 olanlar düşük risktir."),
  cap("Tablo 2 — Risk matrisi"),
  ...table(["", "Etki düşük (1)", "Etki orta (2)", "Etki yüksek (3)"], matrixRows, [25, 25, 25, 25],
    (ri, ci) => { if (ci === 0) return undefined; const o = 3 - ri, e = ci; return level(o * e)[1]; }, 19),
  cap("Tablo 3 — Risk kaydı"),
  ...table(["#", "Risk", "O", "E", "Skor", "Erken uyarı", "Önlem"], riskRows, [5, 29, 4, 4, 11, 16, 31],
    (ri, ci, r) => (ci === 4 ? level(Number(r[4].split(" ")[0]))[1] : undefined), 16),

  H1("4. Nasıl İlerlemeliyiz"),
  P("Üç ilke öneriyorum: **önce en riskli varsayımı doğrula**, **katman katman değil dikey dilim halinde ilerle**, **SRS'i dondur ve kodla birlikte güncelle**. Süreler, haftada 10–15 saat yarı zamanlı çalışma varsayımıyla verilen kaba tahminlerdir."),
  cap("Tablo 4 — Önerilen yol haritası"),
  ...table(["Aşama", "Süre", "Kapsam", "Çıkış ölçütü"], [
    ["F0 · Karar turu", "1–2 gün", "Ek E'deki 40 kararı toplu onayla, yalnız kritik 5 kararı (B1, B3, C1, D1, D3) tartış; D1 ve vault konumunu PRD v0.4'e işle; SRS v1.0'ı etiketle (`git tag srs-v1.0`); push", "Açık karar kalmadı"],
    ["F1 · Model fizibilite denemesi", "1 hafta", "Python + `llama-server`; 100 TR + 100 EN örnek (sentetik + elle kontrol); 3 aday model; 4 iş parçacığı, GPU kapalı ölçüm", "**Karar kapısı:** alan doğruluğu ≥ %80 ve ≤ 10 sn/not → devam; değilse iki adımlı çıkarım, daha büyük model veya kapsam daraltma"],
    ["F2 · Yürüyen iskelet (M0)", "2 hafta", "Tauri + Svelte + Rust; vault aç, not düzenle ve kaydet (atomik yazma + dış değişiklik kontrolü); migrasyonlar; i18n iskeleti; GitHub Actions (test + clippy + derleme)", "Uygulama açılıyor, not güvenle kaydediliyor, CI yeşil"],
    ["F3 · Dikey dilim (M1–M2 çekirdeği)", "3 hafta", "Not kapanır → çıkarım → görev paneli + geri al; model yaşam döngüsü; basit bildirim", "**Demo:** 2 dakikalık video; kendi günlük kullanımına başlanır; README güncellenir"],
    ["F4 · Metrikler, hatırlatıcı, tepsi", "2–3 hafta", "MET, TSK hatırlatıcılar, temel tepsi modu", "1 hafta boyunca kendi günlük kullanımında hatasız"],
    ["F5 · Not sistemi + RAG / soru-cevap", "4–6 hafta", "M3 (wikilink, arama) ve M4 (RAG, araçlar)", "Soru-cevap değerlendirme seti geçer"],
    ["F6 · Bakım, rapor, telafi + cilalama", "3–4 hafta", "M5 ve M6", "PRD §10 kabul ölçütleri"],
  ], [18, 10, 44, 28], null, 17),
  P("**Toplam:** yarı zamanlı çalışmayla MVP'ye yaklaşık 4–5 ay. İlk gösterilebilir sonuç, F3 sonunda yaklaşık 6–7 hafta içinde gelir."),

  H1("5. Öneriler"),
  ...B([
    "**Ö1 — Öncelikleri yeniden sınıflandır.** Dikey dilimdeki gereksinimleri \"MVP-çekirdek\", kalanını \"MVP-tam\" olarak ayır. Hedef, \"Yüksek\" oranını yaklaşık %50'ye indirmek.",
    "**Ö2 — Gereksinimleri testlere bağla.** Test adlarında gereksinim kimliği kullan (ör. `fr_ext_016_updates_existing_item`). İzlenebilirlik matrisine test sütunu böylece otomatik eklenebilir.",
    "**Ö3 — Yeni kararları kısa ADR'lerle kaydet.** SRS'i büyütmek yerine `docs/adr/NNNN-baslik.md` dosyaları kullan. SRS'i yalnız davranış değiştiğinde güncelle.",
    "**Ö4 — Varsayılan vault konumunu değiştir.** `%USERPROFILE%\\PLA Vault` yap; vault OneDrive altındaysa kullanıcıyı uyar. Bu bir PRD v0.4 değişikliği.",
    "**Ö5 — Ölçümleri hedef donanıma göre yap.** Geliştirme makinesi (i7-13620H, 16 iş parçacığı, 32 GB, RTX 4050) hedef donanımdan çok güçlü. Performans ölçümlerini 4 iş parçacığı ve GPU kapalıyla yap; mümkünse 8 GB'lık bir makinede doğrula.",
    "**Ö6 — Kendi vault'unla test et, ama kopyasıyla.** Dogfooding'i canlı Obsidian vault'unla değil, onun bir kopyasıyla yap. Veri kaybı riski (R4) ancak kod olgunlaşınca kalkar.",
    "**Ö7 — F1'in çıktısını boşa harcama.** Değerlendirme setini F1'de kur. Aynı set ileride fine-tuning verisinin ve portföyde \"ölçülmüş iyileşme\" hikâyesinin temeli olur.",
    "**Ö8 — Depoyu vitrin gibi tut.** İki commit'i push et, README'yi yeni vizyon ve yol haritasıyla yeniden yaz. Kod birkaç bin satırı geçince code-review-graph'ı kur.",
  ]),

  H1("6. Önümüzdeki 7 Gün"),
  ...B([
    "Ek E kararlarını gözden geçir (hedef: 1 oturum). Ret veya değişiklik varsa SRS v1.1 için not düş.",
    "PRD v0.4: D1 (vault başına veritabanı) ve varsayılan vault konumu (Ö4).",
    "`git tag srs-v1.0`, push, README'nin yeniden yazılması.",
    "F1'e başla: Claude ile 100 TR + 100 EN sentetik not ve beklenen JSON'lar; 3 aday modeli indir; ölçüm betiği.",
    "F1 sonunda karar kapısı toplantısı: model seçimi (TBD-02) ve F2 başlangıcı.",
  ], "num"),
];

const doc = new Document({
  creator: "Ayberk Cerit", title: "PLA — Gidişat, Risk Analizi ve Yol Haritası",
  styles: {
    default: { document: { run: { font: FONT, size: 21, color: INK } } },
    paragraphStyles: [
      { id: "Heading1", name: "Heading 1", basedOn: "Normal", next: "Normal", quickFormat: true, run: { size: 28, bold: true, color: INK },
        paragraph: { spacing: { before: 320, after: 140 }, outlineLevel: 0, keepNext: true, border: { bottom: { style: BorderStyle.SINGLE, size: 6, color: ACCENT, space: 4 } } } },
      { id: "Heading2", name: "Heading 2", basedOn: "Normal", next: "Normal", quickFormat: true, run: { size: 24, bold: true, color: INK }, paragraph: { spacing: { before: 200, after: 100 }, outlineLevel: 1, keepNext: true } },
    ],
  },
  numbering: { config: [
    { reference: "bul", levels: [{ level: 0, format: LevelFormat.BULLET, text: "•", alignment: AlignmentType.LEFT, style: { paragraph: { indent: { left: 460, hanging: 260 } } } }] },
    { reference: "num", levels: [{ level: 0, format: LevelFormat.DECIMAL, text: "%1.", alignment: AlignmentType.LEFT, style: { paragraph: { indent: { left: 460, hanging: 300 } } } }] },
  ] },
  sections: [{
    properties: { page: { size: { width: 11906, height: 16838 }, margin: { top: 1300, bottom: 1300, left: 1417, right: 1417 } } },
    headers: { default: new Header({ children: [new Paragraph({ alignment: AlignmentType.RIGHT, children: [new TextRun({ text: "PLA · Gidişat, Risk Analizi ve Yol Haritası · 27.09.2026", size: 16, color: SOFT })] })] }) },
    footers: { default: new Footer({ children: [new Paragraph({ alignment: AlignmentType.CENTER, children: [new TextRun({ text: "Sayfa ", size: 16, color: SOFT }), new TextRun({ children: [PageNumber.CURRENT], size: 16, color: SOFT }), new TextRun({ text: " / ", size: 16, color: SOFT }), new TextRun({ children: [PageNumber.TOTAL_PAGES], size: 16, color: SOFT })] })] }) },
    children,
  }],
});

const out = path.join(__dirname, "..", "..", "PLA_Degerlendirme_Risk_Yol_Haritasi.docx");
Packer.toBuffer(doc).then((b) => { fs.writeFileSync(out, b); console.log("yazıldı:", out); });
