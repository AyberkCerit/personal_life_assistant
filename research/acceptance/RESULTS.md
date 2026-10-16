# MVP kabul raporu (M6 parça 3)

Tarih: 16 Ekim 2026 · SRS 1.4 · `main` + `feat/acceptance`. Testler: Rust (pla-core 336, kabuk 109), arayüz
120, gerçek pencere E2E'leri (release derlemesi, Türkçe arayüz). Ölçümler: `research/perf/RESULTS.md`,
doğal dil: `research/nl-eval/RESULTS.md`, model seçimi: `research/f1-model-eval/RESULTS.md`.

## PRD §10 kabul ölçütleri

| # | Ölçüt | Durum | Kanıt |
| --- | --- | --- | --- |
| 1 | Çıkarım kalitesi: JSON %100, doğrulama ≥ %95, alan doğruluğu ≥ %85 (TR ve EN ayrı) | ✓ | F1 değerlendirmesi, 100 TR + 100 EN örnek: JSON %100, anlamsal doğrulama %99,5, alan doğruluğu TR %96 / EN %95 (Q3_K_M). Uygulamanın kendi koduyla çıkarım kümesi 31/31 (nl-eval). Gramer: `prompt.rs::request_body_uses_deterministic_grammar_constrained_settings`. |
| 2 | Tekrar önleme: yazım düzeltmesi yinelenen öğe üretmez; düzenlenen ya da reddedilen öğe yeniden oluşmaz | ✓ | `blocks::a_typo_fix_keeps_the_block`, `duplicates_pair_by_nearest_position`, `pipeline::user_edited_or_completed_items_win`, `rejected_items_are_not_recreated`, `a_reworded_rejected_item_stays_rejected`, `tasks::deleting_an_extracted_task_remembers_the_rejection`, `metrics::a_deleted_extraction_stays_deleted_when_its_note_is_read_again`. |
| 3 | Kaynak: boşta ve çıkarım/soru-cevap sırasında RAM hedefleri | ✓ | Tepside 27 MB (≤ 150), pencere açık 374 MB (≤ 450), model cevaplarken 664 MB (≤ 2 GB); 10 000 notluk referans kasa (perf). |
| 4 | Veri güvenliği: otomatik işlem ham veri silmez; yalnız kasa kopyasından tam geri yükleme | ✓ | Bu aşamada eklendi: `jobs::a_vault_copied_alone_brings_its_tasks_to_another_computer`, `a_backup_comes_back_and_what_was_there_is_kept`, `a_damaged_or_foreign_backup_is_refused_and_nothing_changes`, `recovery::*` (5 test). E2E: kasa başka bir "bilgisayara" (boş APPDATA) kopyalandı, öneri çıktı, görev geri geldi; zorla kapatma + bozuk `pla.db` → dosya silinmeden ayrı tutuldu, yedek önerildi, görev geri geldi; bozuk yedek reddedildi. Silmeme: `pipeline::deleted_note_marks_everything_missing`, `report::a_report_is_never_overwritten`, `engine::instructions_in_a_note_reach_no_tool_outside_the_list_and_no_check_is_skipped`; asistanın silme aracı yok (BR-06). |
| 5 | Güvenilirlik: kapalı geçen Pazar'dan sonra haftalık rapor telafi edilir; kaçırılan hatırlatıcılar açılışta listelenir | ✓ | `scheduler::the_weekly_report_is_asked_for_once_when_due_and_the_user_is_away`, `report::the_due_week_is_the_latest_whose_time_has_come`, `scheduler::reminders_due_before_start_or_during_sleep_are_one_missed_list`, `unanswered_reminders_are_listed_again_after_a_restart`, `worker::catches_up_on_start_and_reports_what_it_added`. Windows oturum açılışında (`--hidden`) oturum açılıyor (M6 parça 1 E2E). |
| 6 | Soru-cevap: ham notlara kaynak gösterir, bilgi yokken uydurmaz, sayılar SQL'le birebir | ✓ | nl-eval asistan kümesi 87/91 (%96; saklı küme 12/15, olumsuz 5/5, sorgu 9/9), `engine::answers_name_their_sources`, `tools::numbers_come_from_the_records`, `report::the_numbers_come_from_the_records`. |
| 7 | Kullanım: geliştirici 4 hafta günlük asıl not/görev aracı olarak kullanır | **Sürüyor** | 16 Ekim – 13 Kasım 2026. Kasada `notes/PLA kullanım günlüğü.md` (günde bir satır) ve dört haftalık rapor. Bu ölçüt dönem bitince kapanır. |

## SRS "Yüksek" gereksinimleri

198 Yüksek gereksinimin 147'si kodda ya da testlerde ID'siyle anılıyor (`research/acceptance/req_gaps.py` taraması; `--all` metinleriyle listeler).
Denetimde anılmayan 61 madde tek tek incelendi:

- **Eksik çıkan ve bu aşamada uygulanan (10):**
  - FR-BKP-004/005/006/008: yedekten geri yükleme, açılışta öneri, başka bilgisayar.
  - FR-LOG-006/007: anormal kapanmadan sonra `pla.db` kontrolü.
  - NFR-REL-001: 1 000 kesintili kaydetme testi.
  - FR-I18N-004: sistem klasörlerinin yerelleştirilmiş adları.
  - NFR-SEC-005: not içindeki talimat testi.
  - FR-EXT-030: etiket eklendi.
- **SRS'ten sapma, SRS 1.4'te düzeltildi (4):**
  - FR-MDL-009/010: llama.cpp CPU arka ucunu çalışma anında seçiyor ve AVX2'siz temel x86-64 arka ucu paketli.
  - FR-MDL-016: embedding ayrı sunucu sürecinde (M4 kararı).
  - FR-LOG-007: bozuk veritabanı salt okunur açılmıyor, silinmeden ayrı tutuluyor (sahibin onayı).
- **Uygulanmış, kanıtı aşağıda (51):**

| Gereksinim | Kanıt |
| --- | --- |
| FR-BKP-007 dışa aktarma | `export::tasks_export_for_excel_with_quoting_and_the_source_note`, `metrics_export_uses_the_regions_separator_and_decimal_mark`, `settings_screen::export_files_are_named_for_the_day_and_language` |
| FR-BKP-009, NFR-MNT-002 göç öncesi yedek, sürümlü göçler | `db::upgrade_backs_up_first_and_newer_schema_is_refused` |
| FR-EDT-001/002 editör, 100 adım geri al | İnceleme: CodeMirror 6 Markdown dili + `history()` (varsayılan derinlik 100) |
| FR-EXT-011 gramer | `prompt::request_body_uses_deterministic_grammar_constrained_settings` |
| FR-EXT-014 `origin = extracted` | `pipeline::a_daily_note_becomes_a_task_once`, `tasks.test.ts › filters by origin` |
| FR-EXT-024, FR-I18N-005/008, NFR-AIQ-001/004 | F1 değerlendirmesi (100 TR + 100 EN), nl-eval, `dates::*` |
| FR-I18N-003 biçimler | `tasks::dates_and_times_are_stored_in_canonical_form`, `export::metrics_export_uses_the_regions_separator_and_decimal_mark`, arayüzde `Intl` |
| FR-LOG-002 loglarda içerik yok | İnceleme: `applog::error` yalnız hata metni yazıyor (8 çağrı) |
| FR-MDL-002, FR-SET-008 onaysız indirme yok | İnceleme: indirme yalnız `model_download_start` düğmesiyle; sihirbaz model kartı ad, boyut, kaynak ve lisansı gösteriyor |
| FR-MDL-006, NFR-SEC-003 SHA-256 | `download::a_checksum_mismatch_deletes_the_file`, `local::rejects_files_that_are_not_gguf` |
| FR-MDL-012 modele yalnız Rust | İnceleme: llama-server 127.0.0.1'e bağlı, rastgele anahtarlı; arayüz yalnız Tauri komutlarını çağırıyor |
| FR-MDL-015 tek dil modeli | İnceleme: oturum başına bir `ModelHost`; perf ölçümü (664 MB) |
| FR-MDL-020/021 8192, flash attention, q8_0 | `llm::args_bind_localhost_with_low_memory_flags_and_no_key` |
| FR-MET-001 ölçüm türleri | `metrics::water_adds_up_and_workouts_count_sessions_and_sets` ve 14 test daha |
| FR-QA-011, NFR-AIQ-002/003 | nl-eval asistan 87/91 (olumsuz ve saklı vakalar dahil) |
| FR-SCH-002 tepsi menüsü, FR-TSK-014 tepside hatırlatıcı | İnceleme `tray::build`; M6 parça 1 E2E (`--hidden` oturum) |
| FR-SCH-006 bakım işi | `jobs::maintenance_runs_once_a_day`, `scheduler::maintenance_waits_for_idle_and_mains_power` |
| FR-SET-002/009/013 sihirbaz, modelsiz bitirme, tek kasa | `settings::setup_is_unfinished_until_the_wizard_says_so`, `wizard.test.ts`; inceleme `AppState.session` |
| FR-TSK-002…005 görev alanları, elle ekleme, düzenleme, tamamlama | `tasks::invalid_input_is_refused_and_changes_nothing`, `user_changes_make_the_task_theirs`, `completing_and_reopening` |
| FR-VLT-002/003/008, NFR-POR-002 Obsidian uyumu, vault_id | `index::links_resolve_like_obsidian_and_backlinks_show_their_line`, `markdown::frontmatter_ends_at_its_closing_fence`, `vault::vault_id_cannot_escape_the_vaults_folder` |
| NFR-MNT-003 model değiştirme | `local::accepts_a_gguf_v3_file_and_reports_its_size` |
| NFR-REL-002/004, NFR-SEC-004 | Ölçüt 4 ve 5'teki testler |
| NFR-REL-003 modelsiz çalışma | `model_paths::missing_files_are_skipped_and_no_model_means_no_config`, `memory::a_note_is_found_by_its_title_without_the_model`, `llm::model_host_does_not_retry_a_missing_model` |
| NFR-SCL-001 10 000 not / 1 GB | `research/perf/RESULTS.md` (942 MB referans kasa) |
| NFR-SEC-006 betik ve uzak kaynak yok | İnceleme: `tauri.conf.json` CSP `default-src 'self'`, `img-src 'self' data: blob:` |
| NFR-USA-002 tek adımda geri alma | `engine::an_undone_note_may_be_written_again`, `tools::a_reminder_is_added_as_the_assistants_and_can_be_undone` |

## Açık kalanlar

- Ölçüt 7 (4 haftalık kullanım) 13 Kasım 2026'da kapanacak.
- Ağaçta sistem klasörleri diskteki İngilizce adlara göre sıralanıyor; Türkçe adlarla alfabetik değil (küçük).
