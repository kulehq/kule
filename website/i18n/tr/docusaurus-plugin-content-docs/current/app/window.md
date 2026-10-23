---
title: Pencere
sidebar_label: Pencere
description: Kule penceresinin bölümleri, her panelin nerede durduğu ve panellerin nasıl taşınıp gizlenip sıfırlandığı.
---

# Pencere

Kule'nin masaüstü uygulaması, Kule daemon'una (arka plan süreci) açılan bir
penceredir. Görevlerinize ve ajanların çalıştırmalarına sahip olan bu arka
plan sürecidir (bkz.
[Pencere kapalıyken](#when-the-window-is-closed)). Bu sayfa pencerenin her
parçasına bir ad verir, böylece diğer sayfalar bu adlarla onlara
başvurabilir.

Bu sayfalarda **Mod**, macOS'ta **⌘**, Linux ve Windows'ta **Ctrl**
anlamına gelir.

## İlk açılış {#first-start}

Henüz hiç görev yokken pencere, *A control room for your agents* başlıklı
bir ilk çalıştırma ekranıyla açılır. Üç adımı vardır:

1. **Your agents (ajanlarınız).** Kule bu bilgisayarda hangi ajanların
   başlayabildiğini denetler (Claude Code, Codex, Gemini ve sizin
   eklediğiniz ajanlar). Başlayamayanlar için neyin kurulması gerektiğini
   söyler. Claude Code ve Codex, `npx` için Node.js'e ihtiyaç duyar;
   Gemini'nin kendi CLI'ı gerekir. Her ajanda bir kez, bir terminalde,
   Kule olmadan nasıl yapıyorsanız öyle oturum açın: Kule'nin
   çalıştırmaları bu oturumu kullanır. Bkz. [Ajanlar](../using-agents.md).
2. **How careful (ne kadar dikkatli).** Tüm görevler için politikayı seçin:
   - **Balanced (dengeli)**: sıradan dosyalardaki değişiklikler
     kendiliğinden geçer. Neyin kurulacağına, derleneceğine ya da
     çalıştırılacağına karar veren dosyalar (manifest'ler, lockfile'lar, CI
     dosyaları, editör ayarları), git geçmişini yeniden yazan işlemler ve
     toplu silmeler sizi bekler.
   - **Careful (dikkatli)**: bir dosyadaki ya da git'teki her değişiklik
     sizi bekler. Sorulmadan yalnızca paket kayıt sunucularına (registry) ve
     çalıştığı sürece her ajanın kendi API'sine erişilir.

   Bunu daha sonra *Settings → Policy* altında değiştirebilirsiniz. Bkz.
   [Politika](../concepts/policy.md).
3. **A folder (bir klasör).** *Choose a folder…* ile seçtiğiniz klasörde
   ilk [görevinizi](../concepts/tasks-and-copies.md) oluşturursunuz.

Uygulama Finder'dan ya da bir başlatıcıdan açıldığında `PATH`'i oturum açma
kabuğunuzdan (login shell) okur. Böylece ajanlar `npx`, `node` ve
diğerlerini bir terminalde bulduğu gibi bulur.

## Bölgeler ve paneller {#regions-and-panels}

Pencerenin dört **bölgesi** vardır: sol, ana, sağ ve alt. Her bölge
**panelleri** sekmeler halinde tutar.

| Panel | Ne gösterir | Varsayılan bölge |
| --- | --- | --- |
| **Tasks** | Görevleriniz ve gruplarınız. En üstteki *Needs you* ("Sizi bekleyenler"), bir yanıt ya da onay bekleyen görevleri toplar. | Sol |
| **Files** | Görevin kopyası bir ağaç olarak; değişen dosyalar işaretli. Bkz. [Files ve Preview](files-and-preview.md). | Sol |
| **Review** | Görevin neyi değiştirdiği ve politikanın bu konuda ne dediği; Commit ve Roll back düğmeleriyle. Bkz. [Review](review.md). | Ana |
| **Timeline** | Commit'ler, adımlar, ağ bağlantıları ve sandbox'ın reddettiği işlemler. Bkz. [Timeline](timeline.md). | Ana |
| **Preview** | Kopyadaki tek bir dosya, büyük gösterilir. Bkz. [Files ve Preview](files-and-preview.md). | Ana |
| **Agent** (Live) | Ajanla konuşma ve istem (prompt) satırı. Bkz. [Live](live.md). | Alt |
| **Terminal** | Kopyada, görevin sandbox'ında açtığınız kendi kabuklarınız, her biri bir sekme. Bkz. [Terminaller ve sunucular](terminals-and-servers.md). | Alt |

Sağ bölge varsayılan olarak boştur ve gizlidir. Varsayılan olarak ajan
sohbeti ve terminaller alt bölgeyi yan yana paylaşır:
`Agent | zsh 1 | zsh 2 | +`.

### Taşıma ve yeniden boyutlandırma {#moving-and-resizing}

- **Bir paneli taşımak**: sekmesini başka bir bölgeye sürükleyin, ya da
  sekmenin menüsünü veya klavyeyi kullanın.
- **Bir bölgeyi yeniden boyutlandırmak**: kenarını sürükleyin. Bir yan
  bölge, ortadaki alan en az 360 × 160 piksel kalana kadar büyüyebilir;
  böylece ana alan iki yan bölgenin arasında hiçbir zaman kaybolmaz.
- **Bir bölgeyi göstermek ya da gizlemek**: sekme çubuğunun sonundaki
  düğmeyi, başlık çubuğunun sağındaki dört anahtarı ya da aşağıdaki
  tuşları kullanın. Alt panel tüm pencereyi kaplayabilir. Ana alan ve alt
  panel hiçbir zaman birlikte gizlenmez.
- **Yerleşimi sıfırlamak**: Kule'nin kendi yerleşimine dönmek için komut
  paletinden (Mod+K) *Reset the layout* komutunu çalıştırın.

| Göster ya da gizle | Tuşlar |
| --- | --- |
| Sol taraf | Mod+B |
| Alt panel | Mod+J |
| Sağ taraf | Mod+Alt+B |
| Ajan sohbeti | Mod+Shift+J |
| Terminal | Ctrl+\` |
| Ana alan | varsayılan yok; *Settings → Shortcuts* altında bir kısayol atayın |

Yerleşim, yeniden başlatmalar arasında Kule'nin ev dizinindeki
`layout.json` dosyasında saklanır (bkz.
[Dosyalar ve ayarlar](../reference/files.md)).

## Başlık çubuğu {#title-bar}

Pencerenin en üstünde şunlar bulunur:

- pencerenin başlığı;
- **Weaker isolation** ("daha zayıf yalıtım"): ajanlara tam yalıtımlarını
  veremeyen bir Linux bilgisayarında daha zayıf yalıtımı kabul ettiğiniz
  sürece görünen kehribar renkli bir etiket (bkz.
  [`kule isolation`](../cli/isolation.md));
- sol taraf, ana alan, alt panel ve sağ taraf için dört **yerleşim
  anahtarı**. macOS'ta bunlar, VS Code'daki gibi pencerenin kendi başlık
  çubuğunda durur.

En soldaki dar çubukta Kule'nin işareti ve en altta **Settings** dişlisi
(Mod+,) bulunur.

## Görev başlığı {#task-header}

Ana alanın üstünde, açık görevin başlığı şunları gösterir:

- görevin adı ve klasörü;
- kopyasında çalışan sunucular için `localhost:5173 ↗` gibi **sunucu
  etiketleri** (bkz. [Terminaller ve sunucular](terminals-and-servers.md));
- "agent at work" ya da "3 changes · needs approval" gibi **durumu**;
- ajanların bildirdiği kadarıyla, çalıştırmalarının şimdiye kadar
  **harcadığı** tutar; çalıştırmalarının harcadıklarına dair bazı kayıtlar
  okunamıyorsa kırmızıyla *N usage records unreadable · budget held* (bkz.
  [Bütçeler ve sınırlar](../concepts/budgets-and-limits.md)). Çalışmakta
  olan ya da bitmeyen çalıştırmalar bir pay tutarken *X set aside* ekler ve
  bunu kalandan düşer;
- klasör, yanındaki bir projeden derleniyor ama görevleri o projeyi henüz
  okuyamıyorsa bir **builds from** etiketi (bkz.
  [`kule reveal`](../cli/reveal.md)).

## Tasks paneli {#the-tasks-panel}

- **New task…** (Mod+N) bir klasör seçer ve onun üzerinde bir görev
  oluşturur.
- **Needs you**, sizi bekleyen görevleri listeler: bir bağlantı hakkında
  bir soru ya da onay gerektiren değişiklikler.
- **Tasks**, tüm görevleri ve grupları listeler. Başlığın yanındaki **+**
  bir [grup](../concepts/groups.md) oluşturur. Kule'nin okuyamadığı bir görev
  listeden çıkmaz; kırmızı bir *cannot be read* etiketi ve nedeniyle listede
  kalır.
- Her görevin bir **⋮ menüsü** vardır (Mod+. ya da sağ tık): *Rename…*,
  bir renk, *Group…*, *Budget…*, *Limits…*, *Open in terminal app*, *Open
  the copy in an editor*, *Bring in the folder's changes*, *Fork task*,
  *Move up*/*Move down* ve *Delete task*. Çalışan bir ajanı rahatsız edecek
  öğeler *(agent at work)* der ve bekler.
- Bir grubun ⋮ menüsünde *Rename…*, bir renk, *Budget…*, *Limits…*, *Move
  up*/*Move down* ve *Delete group* bulunur (grubun görevleri kalır, hiçbir
  grupta olmadan).
- Bir görevi sırasını değiştirmek için sürükleyin ya da oraya koymak için
  bir grubun üzerine bırakın; gruptan çıkarmak için *Not in a group*
  üzerine sürükleyin. Alt+Shift+↑/↓ geçerli görevi taşır (liste odaktayken
  Alt+↑/↓). `kule list` aynı sırayı korur.

## Komut paleti {#command-palette}

**Mod+K** komut paletini açar. Pencerenin her eylemi, varsayılan tuşu
olmayanlar da dahil, adıyla ve kısayoluyla oradadır. Bulmak için adının bir
kısmını yazın. **Mod+/** kısayolları listeler. Bkz. [Klavye](keyboard.md).

## Menü çubuğu {#menu-bar}

File, Edit, View, Task ve Help (macOS'ta ayrıca Kule ve Window); macOS'ta
ekranın, Linux ve Windows'ta pencerenin en üstündedir. Her öğe, kısayoluyla
aynı komutu çalıştırır ve sizin atadığınız tuşları gösterir. Metin
düzenlemenin de kullandığı kısayollar (⌘⌫, ⌥⇧↑) pencerede kalır; böylece bir
metin alanında çalışmaya devam ederler.

## Pencere kapalıyken {#when-the-window-is-closed}

Pencere, **Kule daemon'una** açılan bir görünümdür; çalışan bir daemon
yoksa uygulama onu başlatır. Görevler, ajanların çalıştırmaları ve bu
çalıştırmaların beklediği sorular daemon'a aittir:

- Pencereyi kapatmak hiçbir şeyi durdurmaz. Bir çalıştırma devam eder,
  soruları da süreleri dolana kadar bekler.
- Pencereyi yeniden açtığınızda bu arada olanları görürsünüz.
- Daemon, on dakika boyunca hiçbir şey olmazsa kapanır.

Kule her şeyi kendi ev dizininde, `~/.kule` (ya da `KULE_HOME`) altında
tutar: görevler ve kopyaları, ayarlar, politika ve gizli değerler
(secret'lar). Hiçbir sandbox onu göremez. Bkz.
[`kule daemon`](../cli/daemon.md) ve
[Dosyalar ve ayarlar](../reference/files.md).

## Terminalden {#from-a-terminal}

`kule` komutu, pencereyle aynı görevler üzerinde çalışır. Bkz.
[CLI başvurusu](../cli/index.md); Tasks panelinin yaptıkları için de
[`kule list`](../cli/list.md), [`kule new`](../cli/new.md),
[`kule name`](../cli/name.md) ve [`kule color`](../cli/color.md).
