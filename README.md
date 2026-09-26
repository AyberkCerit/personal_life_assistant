# personal_life_assistant
Personal life assistant application with AI integration.
## 🚀 Yazılım Geliştirme Yaşam Döngüsü (SDLC) ve Proje Yol Haritası

Bu proje, geleneksel şelale (waterfall) modeli yerine, değişen gereksinimlere hızla adapte olabilmek için **Çevik (Agile) - Yinelenmeli ve Artımlı (Iterative & Incremental)** metodoloji kullanılarak yönetilmektedir. Geliştirme süreci 5 ana mühendislik fazına bölünmüştür:

### 🔍 Faz 1: Gereksinim Mühendisliği ve Proje Kapsamı (Requirements & Scoping)
Bu faz, projenin "Neyi, neden çözüyoruz?" sorusunu yanıtlar ve teknik iskeleti hazırlamadan önceki tüm kavramsal tasarımları içerir.
- [x] **Problem ve Çözüm Tanımlaması:** Karmaşık uygulamaların yarattığı bilişsel yükün analizi ve minimalist çözüm tasarımı.
- [x] **Hedef Kitle ve Kapsam:** Birincil personaların belirlenmesi ve MVP (Minimum Viable Product) modüllerinin kilitlenmesi.
[Review PRD](./docs/PRD.md)
- [x] **UML Use Case Analizi:** Sistem sınırlarının ve aktör (User, AI Services) etkileşimlerinin şematize edilmesi.
[Personal Life Assistant Use Case Diyagramı](./docs/images/use-case.svg)
- [ ] **Gereksinim Çıkarımı (Requirement Elicitation):** 7 adımlı standart analiz süreci ile veri toplanması.
- [ ] **EARS Şablonu Entegrasyonu:** Gereksinimlerin kafa karışıklığını önleyecek EARS (Easy Approach to Requirements Syntax) standartlarında yazılması.
- [ ] **Risk Yönetimi Analizi:** Teknik (AI API limitleri) ve operasyonel (veri gizliliği) risklerin belirlenmesi.
- [ ] **SRS (Software Requirements Specification):** Tüm analizlerin projenin "anayasası" olarak tek bir dökümanda toplanması.

### 📐 Faz 2: Sistem Mimarisi ve Teknik Tasarım (System Architecture & UI/UX)
Kodlamaya geçmeden önce, arayüz deneyimi ve veritabanı yapılarının görselleştirilmesi.
- [ ] **Arayüz Tasarımı (UI/UX Wireframes):** Kullanıcının etkileşime gireceği ekranların (Figma/Draw.io) taslaklarının çizilmesi.
- [ ] **Varlık-İlişki Veri Modeli (ER Diagram):** To-Do, Gym ve Health tabloları arasındaki rasyonel veritabanı ilişkilerinin kurulması.
- [ ] **UML Sınıf Diyagramları (Class Diagrams):** Nesne yönelimli (OOP) mimarinin görselleştirilmesi.
- [ ] **Tasarım Desenleri ve Prensipler:** Proje genelinde uygulanacak olan SOLID prensiplerinin ve uygun Tasarım Desenlerinin (Design Patterns) belirlenmesi.

### 💻 Faz 3: Yinelenmeli Geliştirme (Iterative Development)
Planlanan mimarinin çevik sprintler (kısa geliştirme döngüleri) halinde kodlandığı üretim aşaması.
- [x] **Sürüm Kontrol Ortamı:** Git/GitHub repolarının yapılandırılması ve `.gitignore` standartlarının oturtulması.C++ projesi içinde QSqlDatabase sınıfı kullanılarak ana personal_assistant.db SQLite dosyasının yerel bilgisayarda oluşturulması.
- [ ] **Sprint 1 (Çekirdek Modül ve SQLite entegrasyonu):** Kullanıcı oturumu, görev ekleme/silme işlevlerine sahip To-Do altyapısının C++ ve Qt ile geliştirilmesi.
- [ ] **Sprint 2 (Veri Entegrasyonu):** Gym (Antrenman/Set) ve Health (Uyku/Su/Kalori) veritabanlarının oluşturulması ve To-Do listesi ile çift yönlü bağlanması.
- [ ] **Sprint 3 (AI Beyni):** Doğal Dil İşleme (NLP) yetenekleriyle AI (LLM) servislerinin entegrasyonu ve dinam tavsiye/özetleme motorunun kodlanması.

### 🧪 Faz 4: Kalite Güvence ve Test (Quality Assurance & Testing)
Yazılımın güvenilirliğinin ve sınır durumlarının (edge cases) ölçülmesi.
- [ ] **Birim Testleri (Unit Testing):** Yazılan her fonksiyonun (örn: yağ oranı hesaplama, kalori hesabı) izole edilerek test edilmesi.
- [ ] **Entegrasyon Testleri:** Modüllerin (Örn: Gym ile Health modülü) birbiriyle tutarlı veri alışverişi yapıp yapmadığının doğrulanması.
- [ ] **AI Entegrasyon ve Hallucination Testleri:** AI modelinin sisteme girilen To-Do ve Gym verileri dışına çıkmadan, sadece istenilen formatta yanıt verdiğinin test edilmesi.
- [ ] **Kullanıcı Kabul Testleri (UAT):** Geliştirilen uygulamanın MVP gereksinimlerini karşılayıp karşılamadığının test edilmesi.

### 🚀 Faz 5: Yayınlama ve Bakım (Deployment & Maintenance)
Projenin son kullanıcıya ulaştırılması ve sürdürülebilirlik süreçleri.
- [ ] **Paketleme (Deployment):** Masaüstü ortamlar için derlenmiş, kurulabilir (executable) sürümün oluşturulması.
- [ ] **Performans Optimizasyonu:** C++ tarafında olası bellek sızıntılarının (memory leak) kapatılması ve AI yanıt sürelerinin (latency) düşürülmesi.
- [ ] **Geribildirim Döngüsü:** Bakım sürecinde yeni modüllerin planlanması için kullanıcı davranış analizlerinin toplanması.