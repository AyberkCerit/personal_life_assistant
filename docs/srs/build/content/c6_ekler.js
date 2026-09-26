const fs = require("fs");
const path = require("path");

// ---------------------------------------------------------------- gereksinimleri topla (izlenebilirlik + istatistik)
const reqParts = ["c3_ozellikler_a", "c3_ozellikler_b", "c3_ozellikler_c", "c4_arayuzler", "c5_nfr"].map((n) => require("./" + n));
const allReqs = [];
for (const part of reqParts) for (const b of part) if (b.t === "reqs") for (const r of b.rows) allReqs.push({ id: r[0], ears: r[1], pri: r[3], ver: r[4], src: r[5] });

const PRD_TITLES = {
  P1: "§1 Ürün vizyonu ve kapsam", P3: "§3 Teknoloji yığını ve mimari", P4: "§4 Donanım ve performans",
  "P5.1": "§5.1 Vault ve not sistemi", "P5.2": "§5.2 Otonom veri çıkarımı", "P5.3": "§5.3 Görevler ve hatırlatıcılar",
  "P5.4": "§5.4 Sağlık ve verimlilik metrikleri", "P5.5": "§5.5 Soru-cevap ve komut paneli", "P5.6": "§5.6 Zamanlanmış işler",
  P6: "§6 Hiyerarşik hafıza", P7: "§7 Çoklu dil", "P8.1": "§8.1 Model dağıtımı", P9: "§9 Güvenlik ve gizlilik", P10: "§10 Başarı kriterleri",
};
const bySrc = {};
for (const r of allReqs) (bySrc[r.src] = bySrc[r.src] || []).push(r.id);
const prdRows = Object.keys(PRD_TITLES).map((k) => [PRD_TITLES[k], (bySrc[k] || []).join(", ") || "—", String((bySrc[k] || []).length)]);
const otherKeys = Object.keys(bySrc).filter((k) => !PRD_TITLES[k]).sort((a, b) => a.localeCompare(b, "en", { numeric: true }));
const otherRows = otherKeys.map((k) => [k, bySrc[k].join(", "), String(bySrc[k].length)]);

const mods = {};
for (const r of allReqs) {
  const parts = r.id.split("-");
  const key = parts[0] + "-" + parts[1];
  mods[key] = mods[key] || { Y: 0, O: 0, D: 0, n: 0 };
  mods[key][r.pri]++; mods[key].n++;
}
const statRows = Object.entries(mods).map(([k, v]) => [k, String(v.Y), String(v.O), String(v.D), String(v.n)]);
const tot = Object.values(mods).reduce((a, v) => ({ Y: a.Y + v.Y, O: a.O + v.O, D: a.D + v.D, n: a.n + v.n }), { Y: 0, O: 0, D: 0, n: 0 });
statRows.push(["**Toplam**", `**${tot.Y}**`, `**${tot.O}**`, `**${tot.D}**`, `**${tot.n}**`]);
const earsCount = {};
for (const r of allReqs) earsCount[r.ears] = (earsCount[r.ears] || 0) + 1;
const EARS_NAMES = { U: "Her zaman geçerli", E: "Olay güdümlü", S: "Durum güdümlü", W: "İstenmeyen davranış", O: "Opsiyonel özellik", C: "Karmaşık" };
const earsRows = Object.keys(EARS_NAMES).map((k) => [EARS_NAMES[k], String(earsCount[k] || 0)]);

// ---------------------------------------------------------------- inisiyatif kararlarını md'den oku
const md = fs.readFileSync(path.join(__dirname, "..", "..", "INISIYATIF_KARARLARI.md"), "utf8");
const decisionBlocks = [];
let group = null;
for (const line of md.split(/\r?\n/)) {
  const h = line.match(/^## (.+)$/);
  if (h) { group = { title: h[1], rows: [] }; decisionBlocks.push(group); continue; }
  const m = line.match(/^\| ([A-Z]\d+) \| (.+) \| (.+) \| (.+) \|$/);
  if (m && group) group.rows.push([m[1], m[2], m[3], m[4].replace("⏳", "Onay bekliyor").replace("✅", "Onaylandı").replace("❌", "Reddedildi")]);
}
const decisionContent = [];
for (const g of decisionBlocks) {
  decisionContent.push({ t: "h3", x: g.title });
  decisionContent.push({ t: "tbl", head: ["#", "Karar", "Gerekçe", "Durum"], w: [6, 48, 34, 12], mono: [0], size: 17, rows: g.rows });
}

module.exports = [
  { t: "h1", x: "6. Ekler ve Onaylar (Appendices & Approvals)" },

  { t: "h2", x: "Ek A — Onaylar ve Sürüm Geçmişi" },
  { t: "p", x: "Bu belge **inceleme taslağıdır**. Ek E'deki inisiyatif kararları ürün sahibi tarafından değerlendirildikten ve gerekli düzeltmeler yapıldıktan sonra Sürüm 1.0 olarak onaylanır. Sonraki her değişiklik sürüm geçmişine işlenir; onaylanmış bir gereksinimin değiştirilmesi PRD ile tutarlılık kontrolü gerektirir." },
  { t: "sig", rows: [["Ürün sahibi", "Ayberk Cerit"], ["Geliştirici / teknik sorumlu", "Ayberk Cerit"], ["Gözden geçiren / danışman", ""]] },
  { t: "p", x: "Sürüm geçmişi belgenin başındaki \"Sürüm Geçmişi\" tablosunda tutulur." },

  { t: "h1", x: "Ek B — Analiz Diyagramları" },
  { t: "p", x: "Şekil 1–3 Bölüm 2'de yer alır. Bu ekte davranışsal ve veri diyagramları bulunur. Diyagramların kaynak dosyaları (`docs/srs/diagrams/*.html`) düzenlenebilir; PNG'ler `gen.py` ve `export_png.py` ile yeniden üretilir." },
  { t: "h2", x: "B.1 Otonom çıkarım akışı (UML sıralama)" },
  { t: "p", x: "Kullanıcı notu kapattığında değişen bloklar eşleştirilir, ret kayıtları okunur, bloklar referans tarih ve JSON şemasıyla modele gönderilir. Dönen JSON Rust'ta mutlak tarihe çevrilip doğrulanır; geçerliyse öğe eklenip \"Geri al\" bildirimi gösterilir, değilse İnceleme kutusuna yazılır. İlgili gereksinimler: FR-EXT-001…026." },
  { t: "img", f: "pla-cikarim-sirasi.png", cap: "Otonom çıkarım akışı" },
  { t: "h2", x: "B.2 Soru-cevap ve araç çağırma akışı (UML sıralama)", pb: true },
  { t: "p", x: "Soru hibrit aramayla bağlama dönüştürülür; model gerekirse bir araç çağırır, aracın sonucunu Rust SQL ile hesaplar. Yanıt akışla ve ham not bağlantılarıyla gösterilir. İlgili gereksinimler: FR-QA-001…016, FR-MEM-005…010." },
  { t: "img", f: "pla-soru-cevap-sirasi.png", cap: "Soru-cevap ve araç çağırma akışı" },
  { t: "section", landscape: true },
  { t: "h2", x: "B.3 Çıkarılan öğenin yaşam döngüsü (UML durum)" },
  { t: "p", x: "Bir öğe doğrulanır; geçerliyse \"Eklendi (AI)\" durumuna, değilse İnceleme kutusuna geçer. Eklenen öğe blok değiştikçe güncellenebilir; kullanıcı dokunduğunda \"Kullanıcıya ait\" olur ve yapay zekâ bir daha değiştirmez. Geri alınan öğe reddedilir ve imzası saklanır; kaynağı silinen öğe korunur. İlgili gereksinimler: FR-EXT-013…020, BR-02, BR-05." },
  { t: "img", f: "pla-oge-durumlari.png", cap: "Çıkarılan öğenin yaşam döngüsü" },
  { t: "h2", x: "B.4 Model sürecinin yaşam döngüsü (UML durum)", pb: true },
  { t: "p", x: "`llama-server` ilk yapay zekâ isteğiyle başlatılır, sağlık kontrolünden sonra \"Hazır\" olur, 60 saniye boşta kalınca kapatılır. Başlatma hatasında bir kez yeniden denenir, ikinci hatada işler başarısız işaretlenir. İlgili gereksinimler: FR-MDL-011…017." },
  { t: "img", f: "pla-model-sureci.png", cap: "Model sürecinin yaşam döngüsü" },
  { t: "h2", x: "B.5 pla.db mantıksal veri modeli (ER)", pb: true },
  { t: "p", x: "Merkez varlık bloktur: görev, metrik, ret ve inceleme kayıtları kaynak bloklarına bire-çok ilişkiyle bağlanır (elle girilen kayıtlarda `block_id` boştur). İş kayıtları ve soru-cevap geçmişi bağımsız tablolardır. Alanların ayrıntısı Ek C.1'dedir." },
  { t: "img", f: "pla-veri-modeli.png", cap: "pla.db mantıksal veri modeli" },
  { t: "h2", x: "B.6 Veri akış diyagramı (düzey 1)", pb: true },
  { t: "p", x: "Kullanıcı ile dört süreç (not yönetimi, otonom çıkarım, soru-cevap, bakım ve rapor) ve üç veri deposu (vault, `pla.db`, `cache.db`) arasındaki veri akışları. Model deposu yalnız kurulumda kullanıldığı için bu düzeyde gösterilmemiştir." },
  { t: "img", f: "pla-veri-akisi.png", cap: "PLA veri akış diyagramı (düzey 1)" },

  { t: "section", landscape: false },
  { t: "h1", x: "Ek C — Veri Modeli ve Şemalar", pb: false },
  { t: "h2", x: "C.1 pla.db tabloları (kullanıcı verisi)" },
  { t: "tbl", cap: "pla.db tabloları", head: ["Tablo", "Alanlar (anahtar alanlar kalın)", "Not"], w: [18, 52, 30], mono: [0], size: 17, rows: [
    ["block", "**block_id** (TEXT, UUID), note_path, position (INTEGER), text_hash, first_seen_at, last_seen_at, missing (BOOL)", "Nota işaret yazılmaz; bulanık eşleştirmeyle yeniden bağlanır."],
    ["task", "**task_id**, title (≤ 200), details, date (ISO), time (HH:MM), notify_at, status (open/done/cancelled), origin (manual/extracted/assistant), block_id → block, item_signature, user_modified, source_missing, completed_at, created_at, updated_at", "notify_at dolu ise hatırlatıcıdır."],
    ["metric_record", "**metric_id**, type (sleep/weight/steps/water/workout), value_json, unit, date, origin, block_id → block, item_signature, user_modified, source_missing, created_at", "Birleştirme kuralı sorgu katmanında uygulanır."],
    ["rejection", "**rejection_id**, block_id → block, item_signature, rejected_at", "Aynı öğenin yeniden eklenmesini önler."],
    ["review_item", "**review_id**, block_id → block, payload_json, reason, created_at, resolved (BOOL)", "İnceleme kutusu."],
    ["job_run", "**job_type** (daily_maintenance/weekly_report/backup), last_success_at, last_attempt_at, last_error, attempts", "Telafi mantığının dayanağı."],
    ["qa_turn", "**turn_id**, question, answer, tool_calls_json, created_at", "RAG'e girmez; kullanıcı tamamen silebilir."],
    ["schema_version", "**version**, applied_at", "Sürümlü migrasyonlar."],
  ] },
  { t: "p", x: "`item_signature`, öğenin türü ve normalleştirilmiş başlığı/metrik türünden üretilen özettir; aynı bloktan gelen \"aynı öğe\"yi tanımak ve ret kaydıyla karşılaştırmak için kullanılır." },
  { t: "h2", x: "C.2 cache.db tabloları (yeniden üretilebilir)" },
  { t: "tbl", cap: "cache.db tabloları", head: ["Tablo", "Alanlar", "Not"], w: [18, 52, 30], mono: [0], size: 17, rows: [
    ["note_index", "**note_path**, title, aliases, tags, mtime, size, content_hash", "Dış değişiklik tespiti ve hızlı açma."],
    ["note_fts", "FTS5 sanal tablosu: note_path, title, body", "Tam metin arama."],
    ["link", "source_path, target_path, line_text", "Wikilink ve geri bağlantılar."],
    ["chunk", "**chunk_id**, note_path, start_offset, text, token_count, needs_embedding", "200–400 token parçalar."],
    ["chunk_vec", "sqlite-vec `vec0` sanal tablosu: chunk_id, embedding", "Vektör araması."],
    ["daily_summary", "**date**, summary_text, source_paths_json, generated_at, stale (BOOL)", "Yalnız yönlendirme amaçlı."],
  ] },
  { t: "h2", x: "C.3 Çıkarım JSON şeması (v1)" },
  { t: "p", x: "Model çıktısı bu şemadan türetilen gramerle kısıtlanır. Şema MVP boyunca geriye dönük uyumlu tutulur; fine-tuning öncesinde dondurulur (PRD §8.2)." },
  { t: "code", x: [
    "{",
    "  \"type\": \"object\",",
    "  \"required\": [\"items\"],",
    "  \"properties\": {",
    "    \"items\": { \"type\": \"array\", \"maxItems\": 5, \"items\": {",
    "      \"type\": \"object\",",
    "      \"required\": [\"type\"],",
    "      \"properties\": {",
    "        \"type\":    { \"enum\": [\"task\", \"reminder\", \"metric\"] },",
    "        \"title\":   { \"type\": \"string\", \"maxLength\": 200 },",
    "        \"details\": { \"type\": \"string\", \"maxLength\": 500 },",
    "        \"when\": { \"type\": \"object\", \"properties\": {",
    "          \"day_offset\": { \"type\": \"integer\", \"minimum\": -30, \"maximum\": 365 },",
    "          \"weekday\":    { \"enum\": [\"mon\",\"tue\",\"wed\",\"thu\",\"fri\",\"sat\",\"sun\"] },",
    "          \"which\":      { \"enum\": [\"this\", \"next\"] },",
    "          \"date\":       { \"type\": \"string\", \"pattern\": \"^\\\\d{4}-\\\\d{2}-\\\\d{2}$\" },",
    "          \"time\":       { \"type\": \"string\", \"pattern\": \"^\\\\d{2}:\\\\d{2}$\" },",
    "          \"notify_before_min\": { \"type\": \"integer\", \"minimum\": 0 } } },",
    "        \"metric\": { \"type\": \"object\", \"properties\": {",
    "          \"kind\":  { \"enum\": [\"sleep\", \"weight\", \"steps\", \"water\", \"workout\"] },",
    "          \"value\": { \"type\": \"number\" },",
    "          \"unit\":  { \"enum\": [\"h\", \"kg\", \"lb\", \"count\", \"ml\", \"glass\"] },",
    "          \"exercise\": { \"type\": \"string\" },",
    "          \"sets\": { \"type\": \"integer\" }, \"reps\": { \"type\": \"integer\" } } }",
    "      } } }",
    "  }",
    "}",
  ].join("\n") },
  { t: "p", x: "Anlamsal doğrulama kuralları: `task`/`reminder` için `title` zorunludur; `metric` için `metric.kind` ve `metric.value` zorunludur ve §3.8.1'deki makul aralıkta olmalıdır; `when` içinde `day_offset`, `weekday` + `which` ve `date`'ten en çok biri bulunabilir; `time` 00:00–23:59 aralığında olmalıdır." },
  { t: "h2", x: "C.4 Asistan araç şemaları (özet)" },
  { t: "tbl", cap: "Araç parametreleri", head: ["Araç", "Parametreler", "Dönüş"], w: [20, 45, 35], mono: [0], size: 17, rows: [
    ["search_notes", "query (string), limit (1–10, varsayılan 5)", "[{note_path, snippet, score}]"],
    ["query_tasks", "from, to (ISO tarih), status?, origin?, count_only (bool)", "Görev listesi veya sayı"],
    ["query_metrics", "kind, from, to, stats: [sum, avg, min, max, trend]", "Günlük değerler ve istenen istatistikler"],
    ["add_task", "title, date?, time?, notify_before_min?, details?", "Oluşturulan task_id"],
    ["complete_task", "task_id", "Güncel durum"],
    ["log_metric", "kind, value, unit, date?, exercise?, sets?, reps?", "Oluşturulan metric_id"],
    ["create_note", "title, body (≤ 10 000 karakter)", "Oluşturulan note_path"],
  ] },
  { t: "h2", x: "C.5 Vault meta verisi" },
  { t: "tbl", cap: "`.pla/config` ve frontmatter anahtarları", head: ["Konum", "Anahtar", "Anlamı"], w: [18, 30, 52], mono: [1], size: 17, rows: [
    ["`.pla/config`", "vault_id", "Vault'un veritabanlarını bulmak için rastgele kimlik"],
    ["`.pla/config`", "folders.daily, folders.inbox, folders.notes, folders.reports, folders.attachments, folders.templates", "Sistem klasörü yolları"],
    ["`.pla/config`", "schema_version", "Vault yapılandırma sürümü"],
    ["Frontmatter", "tags, aliases", "Obsidian standart alanları (okunur, korunur)"],
    ["Frontmatter", "pla_generated", "true ise not yapay zekâ tarafından üretilmiştir; çıkarıma girmez"],
  ] },

  { t: "h1", x: "Ek D — İzlenebilirlik Matrisi ve Açık Konular" },
  { t: "h2", x: "D.1 PRD → SRS izlenebilirliği" },
  { t: "p", x: "Aşağıdaki tablo gereksinim tablolarındaki \"Kaynak\" sütunundan otomatik üretilmiştir." },
  { t: "tbl", cap: "PRD bölümleri ve karşılayan gereksinimler", head: ["PRD bölümü", "SRS gereksinimleri", "Adet"], w: [24, 68, 8], size: 16, rows: prdRows },
  { t: "tbl", cap: "Diğer kaynaklar (inisiyatif kararları, iş kuralları, varsayımlar, referanslar)", head: ["Kaynak", "SRS gereksinimleri", "Adet"], w: [14, 78, 8], mono: [0], size: 16, rows: otherRows },
  { t: "h2", x: "D.2 PRD başarı kriterleri → gereksinimler" },
  { t: "tbl", cap: "Kabul ölçütlerinin eşlenmesi", head: ["PRD §10 kriteri", "Doğrulayan gereksinimler"], w: [45, 55], size: 17, rows: [
    ["1. Çıkarım kalitesi", "NFR-AIQ-001, NFR-AIQ-004, FR-EXT-011, FR-EXT-013"],
    ["2. Tekrar önleme", "FR-EXT-006, FR-EXT-016, FR-EXT-017, FR-EXT-019"],
    ["3. Kaynak (RAM) hedefleri", "NFR-PERF-006, NFR-PERF-007, FR-MDL-013, FR-MDL-015"],
    ["4. Veri güvenliği ve tam geri yükleme", "NFR-REL-002, FR-BKP-001, FR-BKP-008, FR-EXT-021"],
    ["5. Telafi ve kaçırılan hatırlatıcılar", "FR-SCH-012, FR-TSK-016, NFR-REL-004"],
    ["6. Soru-cevap kaynak gösterme, uydurmama", "FR-QA-010, FR-QA-011, FR-QA-007, NFR-AIQ-003"],
    ["7. 4 haftalık günlük kullanım", "Kabul testi (gösterim); tüm Yüksek öncelikli gereksinimler"],
  ] },
  { t: "h2", x: "D.3 Gereksinim istatistikleri" },
  { t: "tbl", cap: "Modül ve önceliğe göre gereksinim sayıları", head: ["Modül", "Yüksek", "Orta", "Düşük", "Toplam"], w: [28, 18, 18, 18, 18], mono: [0], size: 17, rows: statRows },
  { t: "tbl", cap: "EARS şablonlarının dağılımı", head: ["EARS şablonu", "Gereksinim sayısı"], w: [60, 40], size: 17, rows: earsRows },
  { t: "h2", x: "D.4 Açık konular (TBD)" },
  { t: "p", x: "Aşağıdaki konuların hiçbiri gereksinimleri engellemez; tasarım veya ölçüm aşamasında kapatılacaktır." },
  { t: "tbl", cap: "Açık konular", head: ["Kod", "Konu", "Etkilediği", "Kapanış"], w: [10, 44, 24, 22], mono: [0], size: 17, rows: [
    ["TBD-01", "Kesin ürün adı", "Belge başlıkları, kurulum paketi", "M6 öncesi"],
    ["TBD-02", "Dil modeli ve embedding modeli seçimi", "NFR-AIQ-*, NFR-PERF-007…010", "M1 değerlendirmesi"],
    ["TBD-03", "GPU hızlandırmasının kapsamı (Vulkan/CUDA)", "FR-MDL-018", "M1 sonrası"],
    ["TBD-04", "Bulanık blok eşleştirme algoritması ve benzerlik eşiği", "FR-EXT-006, FR-EXT-016", "M2 tasarım belgesi"],
    ["TBD-05", "Günlük not ve haftalık rapor şablonlarının içeriği", "FR-VLT-010, FR-SCH-009", "M2 / M5"],
    ["TBD-06", "IPC komutlarının ayrıntılı imzaları", "IR-SW-001", "M0 tasarım belgesi"],
    ["TBD-07", "Kod imzalama sertifikası", "NFR-SEC-009", "M6"],
    ["TBD-08", "Referans veri setinin (10 000 not, 5 yıllık veri) sentetik üretimi", "§5 ölçümleri", "M1"],
    ["TBD-09", "Parça boyutu, arama sonuç sayısı (k) ve bağlam paylarının ince ayarı", "FR-MEM-001, FR-MEM-009", "M4 ölçümü"],
  ] },

  { t: "h1", x: "Ek E — İnisiyatifle Alınan Kararlar (Onay Bekleyen)" },
  { t: "p", x: "Bu SRS yazılırken PRD'de açık kalan veya hiç ele alınmamış konularda aşağıdaki kararlar ürün sahibine sorulmadan, önerilen varsayılan olarak alınmıştır. Her karar onay bekler; reddedilen karar SRS'te ve gerekirse PRD'de düzeltilir. Kararların güncel durumu `docs/srs/INISIYATIF_KARARLARI.md` dosyasında tutulur ve bu ek oradan üretilir. Gereksinim tablolarındaki `E-<kod>` kaynakları bu kararlara işaret eder." },
  ...decisionContent,

  { t: "h1", x: "Ek F — Üçüncü Parti Bileşenler ve Lisanslar" },
  { t: "p", x: "Uygulamaya yalnız izin verici lisanslı bileşenler dahil edilir (CON-12). Aşağıdaki sürümler ve lisanslar planlama aşamasındaki bilgidir; her bileşen sürüm sabitlenirken lisans dosyasıyla yeniden doğrulanacaktır. Ticari sözleşme veya ücretli lisans gerektiren bir bileşen yoktur." },
  { t: "tbl", cap: "Uygulamayla dağıtılan bileşenler", head: ["Bileşen", "Kullanım", "Lisans"], w: [28, 42, 30], size: 17, rows: [
    ["Tauri 2 ve resmi eklentileri", "Uygulama çatısı, IPC, bildirim, otomatik başlatma, diyalog, tek örnek", "MIT veya Apache-2.0"],
    ["Svelte 5", "Arayüz çatısı", "MIT"],
    ["CodeMirror 6", "Markdown editörü", "MIT"],
    ["uPlot", "Metrik grafikleri", "MIT"],
    ["svelte-i18n", "Arayüz çevirileri", "MIT"],
    ["Rust standart kütüphanesi", "Çekirdek", "MIT veya Apache-2.0"],
    ["tokio", "Asenkron çalışma zamanı, zamanlayıcı", "MIT"],
    ["serde, serde_json", "JSON işleme", "MIT veya Apache-2.0"],
    ["rusqlite (gömülü SQLite)", "Veritabanı erişimi", "MIT (SQLite: Public Domain)"],
    ["sqlite-vec", "Vektör araması", "MIT veya Apache-2.0"],
    ["llama.cpp (`llama-server`)", "Dil modeli çıkarımı", "MIT"],
    ["fastembed-rs", "Süreç içi embedding", "Apache-2.0"],
    ["ONNX Runtime", "Embedding modelinin çalıştırılması", "MIT"],
    ["reqwest + rustls", "Model indirme (HTTPS)", "MIT veya Apache-2.0 (rustls: Apache-2.0 / ISC / MIT)"],
    ["chrono", "Tarih/saat hesapları", "MIT veya Apache-2.0"],
    ["sha2", "SHA-256 doğrulaması", "MIT veya Apache-2.0"],
    ["trash", "Geri Dönüşüm Kutusu'na taşıma", "MIT"],
    ["Arayüz yazı tipi (ör. Inter)", "Yerel olarak paketlenen font", "SIL Open Font License 1.1"],
  ] },
  { t: "tbl", cap: "İndirilen modeller (uygulamayla dağıtılmaz)", head: ["Aday model", "Kullanım", "Lisans (doğrulanacak)", "Not"], w: [26, 18, 26, 30], size: 17, rows: [
    ["Qwen2.5 1.5B Instruct (GGUF)", "Dil modeli adayı", "Apache-2.0", "Qwen2.5 3B farklı (Qwen Research) lisanslıdır; ticari kullanım kısıtlıdır."],
    ["Qwen3 küçük modeller (GGUF)", "Dil modeli adayı", "Apache-2.0", "Değerlendirme setiyle karşılaştırılacak."],
    ["Gemma 3 küçük modeller", "Dil modeli adayı", "Gemma Kullanım Şartları", "Kullanım politikası var; indirmeden önce gösterilmeli."],
    ["Llama 3.2 1B / 3B", "Dil modeli adayı", "Llama 3.2 Community License", "Kullanım politikası ve atıf şartı var."],
    ["multilingual-e5-small (ONNX)", "Embedding modeli", "MIT", "Çok dilli, küçük."],
  ] },
  { t: "p", x: "**Yükümlülükler:** MIT ve Apache-2.0 bileşenlerin telif ve lisans metinleri ile Apache NOTICE dosyaları Hakkında bölümünde listelenir (FR-SET-017). Model lisansı, indirmeden önce kullanıcıya gösterilir (FR-SET-008). Değerlendirme ve fine-tuning için kullanılan Python araçları (Unsloth, Hugging Face TRL) uygulamayla dağıtılmaz." },

  { t: "h1", x: "Ek G — Gelecek Kapsam (V2 ve sonrası)" },
  { t: "p", x: "Aşağıdaki özellikler bilinçli olarak MVP dışında bırakılmıştır; gereksinimleri V2 planlamasında yazılacaktır." },
  { t: "bul", items: [
    "Uygulama içi şifreleme (ör. SQLCipher) ve isteğe bağlı uygulama kilidi.",
    "Obsidian ile canlı eşitleme için dosya izleyici.",
    "Notlardaki `- [ ]` onay kutularının görevlerle iki yönlü eşitlenmesi; tekrarlayan görevler.",
    "Çıkarım için ayardan \"onay modu\" (otomatik ekleme yerine öneri + onay).",
    "Pil ve tam ekran algılamasıyla arka plan işlerinin ertelenmesi.",
    "Modelin çıkarım görevine LoRA/QLoRA ile uyarlanması (PRD §8.2, M7 — koşullu).",
    "macOS ve Linux desteği; mobil platformlar.",
    "Giyilebilir cihaz entegrasyonu, sesli komut.",
    "Soru-cevap geçmişinin isteğe bağlı olarak hafızaya (RAG) dahil edilmesi.",
  ] },
];
