# Asistan hafızası: okuma yolunun ölçümü (Aşama 1 kapısı)

Soru: hafıza dosyalarını (`memory/`) istemin içine koymak, notlarda olmayan ama hafızada olan soruları
cevaplatır mı ve notlardan bulunan bağlamı bozar mı?

## Nasıl çalıştırılır

```bash
$env:PLA_LLAMA_SERVER = "C:\dev\PLA\research\f1-model-eval\bin\llama-server.exe"
$env:PLA_EMBED_MODEL = "$env:LOCALAPPDATA\PLA\models\embeddinggemma-300M-Q8_0.gguf"
$env:PLA_MODEL = "C:\dev\PLA\research\f1-model-eval\models\gemma-4-E2B-it-Q3_K_M.gguf"
# arama (hızlı, ~1 dk)
cargo run --release -p pla-core --example retrieval_eval -- memory-p1
cargo run --release -p pla-core --example retrieval_eval -- memory-p1-nomem --no-memory
# asistan (model gerekir)
cargo run --release -p pla-core --example nl_eval -- <etiket> [id-öneki] [--no-memory]
```

`--no-memory` hafızayı dosyaya yazmaz ve yüklemez (boş `Recall`). `nl_eval` vakadaki `memory` alanını
geçici kasanın hafıza klasörüne `recall::render` ile yazar, `recall::load` ile yükler ve
`Question.recall`'a verir. `contains_any` ikinci bir "herhangi biri" grubudur; `contains` ile birlikte
verilirse ikisi de tutmalıdır.

## Dondurulan vakalar (commit `test(recall): freeze the memory eval cases`)

- `retrieval/vault.json`: 6 `none` vakası (IBAN, uçuş saati, tarif, vault'ta olmayan kişi, wifi şifresi,
  ehliyet sonucu; `facts: []`) ve 507 tokenlık bir `memory` fixture'ı (5 hakkımda, 3 kural, 3 durum,
  Deniz ve Efe için 3 kişi satırı). Fixture, mevcut 22 vakanın hiçbir olgusunu içermez.
- `nl/assistant.jsonl`: 8 geliştirme (`cat: memory`) ve 5 saklı (`cat: heldout`, id `memory-h-*`)
  vaka. Cevap yalnız hafızada: kişi olgusu (Deniz, Efe, Cenk), alışkanlık/saat (spor, laboratuvar günü,
  ulaşım, kahvaltı), alerji, telefon, kullanıcı kuralı (hitap, "Kolay gelsin") ve iki çıkarım durumu
  (`contains_any: görünüyor / notlarına göre / notlarında`). Uzunluk sınırı için düzenekte denetim
  olmadığından "kısa cevap" kuralı vakası yazılmadı. Saklı vakalar örnek bankasına Jaccard 0,5
  denetiminden geçti (düzenek başlangıçta denetler; koşu hata vermedi).

## Arama sonucu (EmbeddingGemma, `memory-p1c` ve `memory-p1c-nomem`)

Bütçe payları: son 48 saat + hafıza bloğu birlikte eski %40'ı oluşturur (`RECENT_TOKENS` + `MEMORY_TOKENS`,
her biri sabit); bulunan payı hafızadan önceki değerindedir, yalnız anılan kişilerin bloğu kadar küçülür.
Bu fixture'da hafıza bloğu 507 token.

| Mod | Hafızasız (Bağlı / Doğrudan) | Hafızalı (Bağlı / Doğrudan) | Ort. bulunan token (hafızasız → hafızalı) |
| --- | --- | --- | --- |
| kelime | 7/12 · 10/10 | 7/12 · 10/10 | 1143 → 1143 |
| kelime + bağ | 10/12 · 10/10 | 10/12 · 10/10 | 1328 → 1328 |
| hibrit | 10/12 · 10/10 | 10/12 · 10/10 | 1316 → 1316 |
| hibrit + bağ | 11/12 · 10/10 | 11/12 · 10/10 | 1472 → 1472 |

Hafızalı ve hafızasız koşu aynı: hibrit + bağ 21/22, hafıza bulunan notlara hiçbir şey kaybettirmiyor.
Doğrudan vakalarda gerileme yok. (`memory-p1.jsonl` ve `memory-p1-nomem.jsonl` dengelemeden önceki
sonuçlardır; güncel sonuç `memory-p1c*.jsonl`.)

### `none` vakaları (cevap vault'ta yok, 6 vaka)

| Mod | Ort. bulunan token | Ort. kaynak not |
| --- | --- | --- |
| kelime | 443 | 5,0 |
| kelime + bağ | 600 | 6,0 |
| hibrit | 1204 | 12,0 |
| hibrit + bağ | 1349 | 13,0 |

Hafızalı ve hafızasız aynı. Cevabı olmayan sorularda model hibrit modda yaklaşık 12 not ve 1200+ token okuyor.

### Geçmiş: dengelemenin nedeni

İlk uygulamada hafıza bloğu bulunan payın içinden harcanıyordu (`memory-p1`): hibrit + bağ **20/22**,
`linked-iha-ucuz` "6.800" olgusunu kaybetti, ort. bulunan token 1472 → 1341. Bu yüzden hafıza son 48
saat payının yanına, kendi diliminde alındı (karar L, düzeltme M); sonuç yukarıdaki tablo.

## Duman testi (gerçek model, tek vaka)

`memory-deniz-kahve`, Gemma 4 E2B Q3_K_M:

| Koşu | Sonuç | Cevap |
| --- | --- | --- |
| hafızalı | PASS (21,8 sn) | "Deniz kahveyi sütsüz ve şekersiz içer." |
| `--no-memory` | FAIL (19,9 sn): cevapta "sütsüz" yok | |

Düzenek hafızayı yazıyor, yüklüyor ve cevap yalnız hafızayla çıkıyor.

## Model koşuları

Gemma 4 E2B Q3_K_M. Hafıza vakalarında istem dört turda ayarlandı (`memory-p1`…`p4`, 2026-10-19; dosyalar
orijinal worktree'de kaldı, commit edilmedi):

| Tur | Hafızasız | Hafızalı |
| --- | --- | --- |
| p1 | 0/13 | 8/13 |
| p2 | 0/13 | 9/13 |
| p3 | 0/13 | 10/13 |
| p4 | 0/13 | 9/13 |

### Kapı koşusu (`memory-p5-all`, `memory-p5-all-nomem`, 2026-10-23)

Aynı kod (PR #10 dalı, main ile birleştirilmiş, `7bb5f98`), 152 vakanın tamamı:

| | Hafızasız | Hafızalı |
| --- | --- | --- |
| Hafıza vakaları (13) | 0/13 | 9/13 |
| Eski vakalar (138) | 122/138 | 121/138 |
| Toplam | 123/152 (40 dk) | 131/152 (64 dk) |

- Hafıza vakaları hafızasız hiç geçmiyor, hafızalı 9/13 geçiyor.
- Eski vakalarda iki koşu arasındaki tek fark `lang-thanks` (hafızalıda İngilizce soruya Türkçe cevap).
  Hafızalı koşuda yeni geçen eski vaka yok.
- Kalan 4 hafıza hatası: iki çıkarım vakası (`memory-bitirme-govde`, `memory-h-kabuk`) çekincesiz
  cevaplanıyor; iki saklı vaka (`memory-h-cenk-oyun`, `memory-h-telefon`) hafızada olduğu halde "notlarda
  bulamadım" diyor.
- Hafızalı koşu 1,6 kat uzun sürdü (hafıza bloğu her istemde ~500 token). Gerçek uygulamada istem
  önbelleği bunu büyük ölçüde karşılamalı, `qa_probe` ile ölçülmedi.

**Kapı: geçti.** Hafızanın kendisi eski vakalarda gerileme yapmıyor (fark 1 vaka, dil vakası).

### Ayrı bulgu: main'deki dil gerilemesi

10-19 koşusunda (`memory-p3-all`, PR #4 öncesi kod) geçen 11 İngilizce vaka bu koşuda **iki modda da**
kalıyor: İngilizce soruya Türkçe ("Senin…", "Sen…") cevap (`metric-*-en`, `task-rent-en`, `note-save-en`,
`query-*-en`, `heldout-scale-en`, `heldout-sleep-avg`, `lang-thanks`). Neden PR #4'ün (asistan adı/hitap)
sistem istemine her zaman eklediği "In Turkish, speak to the user as sen" cümlesi: varsayılan (sen)
ayarda bile eklendiği için küçük model bunu Türkçe cevap işareti olarak okuyor. Hafızayla ilgisi yok,
main'de de aynı kod var.

- [ ] `qa_probe -- <ref>` hafızalı ve hafızasız: ilk cevap süresi ve `cache_n`

## Sohbetten hafızaya yazma (Aşama 2, Task 8)

Soru: sohbet adımı (`recall::extract::request` → model → `parse` → `guard`) yalnız kullanıcının kalıcı
olarak söylediğini mi yazıyor?

```bash
cargo run --release -p pla-core --example nl_eval -- <etiket> memory          # 42 vaka
cargo run --release -p pla-core --example nl_eval -- <etiket> chat-mem-d      # yalnız geliştirme
```

`assistant.jsonl`'deki Task 5 vakaları da `memory-` ile başladığı için bu vakaların kimliği `chat-mem-`,
dosya adı `memory` ile seçilir (plandaki `memory-` öneki ikisini birden koşardı).

### Dondurulan vakalar (commit `test(recall): freeze the memory extraction cases`)

`tests/nl/memory.jsonl`, istem ayarlanmadan önce: 30 geliştirme, 12 saklı. Kendisi (alışkanlık, sağlık,
iş, kural, proje durumu), 10 kişi vakası (ikisi aynı mesajda iki kişi, biri akrabalık eki "Annemin",
ikisi İngilizce), yazılmaması gereken 14 vaka (olumsuz, varsayım, soru, dilek, koşul, plan, yalnız
bugüne ait, asistanın önerisine "tamam"), konu güncelleme, iki "unut", reddedilen satırın yanında yeni
bilgi, "bunu hatırla" ile önceki mesaj. Saklı vakalar geliştirme vakalarına ve istemdeki örneklere
Jaccard 0,5 denetiminden geçer (düzenek her koşuda başta denetler).

- **Yanlış yazım:** guard'dan geçip beklenen hiçbir kayda uymayan her `Add` ya da `Forget`.
- **Doğru yazım:** beklenen kayıtlardan bulunanlar.

### Ayar turları (yalnız geliştirme kümesi, 2026-10-24)

| Tur | Vaka | Doğru yazım | Yanlış yazım | Değişiklik |
| --- | --- | --- | --- | --- |
| v1 | 17/30 | 8/21 | 2 | Task 7 istemi, örneksiz. Model her kaydı `people` dosyasına, kişiyi "User" diye yazdı; konu genel ("facts-and-preferences"). |
| v2 | 16/30 | 11/21 | 12 | 7 örnek (few-shot), alan sırası `evidence, file, person, topic, text`, `max_tokens` 600 (v1'de iki cevap kesildi). Artık fazla yazıyor. |
| v3 | 22/30 | 14/21 | 4 | Guard: bugüne/haftaya ait ve gelecek zamanlı cümle, unutma isteği, `person` doluysa Kişiler. Akrabalık sözcüğü ekli eşleşir. Kullanıcı turunda "Write text in …". |
| v4 | 24/30 | 15/21 | 1 | Zaman kontrolü yan cümle düzeyinde; "akşam" `-sam` koşul eki sayılmıyordu (d28'i v2'den beri düşüren yanlış pozitif). Alıntı kök eşleşmesiyle de kabul (bütün mesaj okunur). Dil uyuşmazlığı, "yapalım"/"let", unut isteğinde önceki mesaj. |
| v5 | 28/30 | 19/21 | 0 | Yanlış dildeki satır yerine kullanıcının kendi sözü; model kişiyi boş bırakınca akrabalık sözcüğünden ad ("Annemin" → "Annem"); rutin örneği. |

### Son koşu (`memory-v5`, saklı küme ilk ve tek kez)

| Küme | Vaka | Doğru yazım | Yanlış yazım |
| --- | --- | --- | --- |
| Geliştirme | 28/30 | 19/21 (%90) | 0 |
| Saklı | 11/12 | 8/9 (%89) | **0** |

**Kapı: geçti** (saklıda yanlış yazım 0, geliştirmede doğru yazım ≥ %80).

Kalanlar, üçü de yazmama yönünde:
- `chat-mem-d27`: "Bunu unut" (önceki mesaj) konuyu unutmuyor; model `forget` yerine iki kayıt yazıyor, guard ikisini de atıyor.
- `chat-mem-d28`: model iki bilgiyi tek kayıtta birleştirip bütün cümleyi kanıt veriyor; "Bugün" geçtiği için atılıyor.
- `chat-mem-h03` ("Kod örneklerini Rust ile yaz lütfen."): model boş döndü.

Notlar:
- İngilizce mesajlara model örneklere rağmen Türkçe metin yazıyor; guard o zaman kullanıcının sözünü olduğu gibi yazar ("I work night shifts at the hospital.").
- Reddedilene Jaccard ≥ 0,5 olan öneri, kullanıcı aynı şeyi açıkça yeniden söylese de atılıyor (Task 7 testi). Spec "kullanıcı yeniden söylerse yazılır" diyor; bu vakalarda o durum yok, karar açık.
- Asistanın sohbet ve çıkarım istemleri değişmedi; eski 138 nl-eval vakası bu kodu kullanmaz, yeniden koşulmadı.
