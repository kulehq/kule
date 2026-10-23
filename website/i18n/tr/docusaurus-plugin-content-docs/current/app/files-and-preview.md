---
title: Files ve Preview
sidebar_label: Files ve Preview
description: Bir görevin kopyasında gezinin; metin, kod, resim, tablo ve belgeleri önizleyin, dosyaları Kule dışında açın ve ajana bir dosyadaki belirli bir yeri sorun.
---

# Files ve Preview

**Files**, görevin kopyasını, yani ajanın çalıştığı klasörü gösterir;
**Preview** ise bu kopyadaki dosyalardan birini büyük gösterir. İkisi de
gerçek klasörünüzü değil, kopyanın şimdiki halini gösterir. İkisi de hiçbir
zaman bir bağlantıyı (link) izlemez ya da kopyanın dışına çıkmaz.

## Files {#files}

**Files** (Mod+Shift+E), görevin kopyasını bir ağaç olarak gösterir:

- her dosya ve klasörün, VS Code'un Material Icon Theme'inin çizdiği gibi
  bir simgesi vardır (bir Rust dosyası, bir `package.json`, bir `src`
  klasörü, …);
- görevin değiştirdiği dosyalar işaretlidir;
- en üstteki düğme, kopyanın tamamını Finder'da (Linux'ta Files'da)
  gösterir.

Bir dosyayı Preview'da açmak için üzerine tıklayın. [Kule dışında](#outside-kule)
eylemleri için bir dosyaya ya da klasöre sağ tıklayın.

## Preview {#preview}

**Preview** (Mod+4), seçilen dosyayı gösterir:

| Tür | Nasıl gösterilir |
| --- | --- |
| Metin ve kod | Bir editördeki gibi satır numaraları ve renklerle: 53 dil; 300 KB'a kadar renklendirilmiş, 1 MB'a kadar gösterilir. |
| Bir iki satıra sıkıştırılmış JSON ya da XML | *Formatted* onu düzenli gösterir (1 MB'a kadar). |
| Markdown | Biçimlendirilmiş olarak ya da kaynağıyla. |
| PNG, JPEG, GIF, WebP | Panele sığdırılmış ya da %100 boyutunda. |
| CSV ve Excel (`.xlsx`) | A1'den başlayan bir tablo; her sayfa bir sekme. Bir çubuk, seçilen hücrenin değerini ya da formülünü gösterir. Formüller hesaplanmaz: hücre, Excel'in onunla birlikte kaydettiği değeri gösterir. |
| Word (`.docx`) | Okumak için başlıklar, paragraflar, numaralı listeler ve tablolar. Biçimlendirme, resimler ve notlar dışarıda bırakılır. |
| Diğer her şey | Boyutu ve neden gösterilmediği. |

Renkler kullanılan temaya uyar. Excel ve Word dosyaları bellekte okunur,
hiçbir zaman diske açılmaz; her parça yalnızca bir üst sınıra kadar açılır
(inflate). Bir belge, tablo ya da CSV önizlemesi en çok 16 MB tutar; ötesinde
sığanı gösterir ve kesildiğini söyler. 256 düzeyden derin iç içe JSON ya da
XML biçimlendirilmeden, yazıldığı gibi gösterilir. Kule'nin dizemediği bir
Markdown dosyası düz metin olarak gösterilir.

## Kule dışında {#outside-kule}

Files'da bir dosyaya ya da klasöre sağ tıklayın veya şu komutları kullanın:

| Eylem | macOS | Linux |
| --- | --- | --- |
| **Open with its app** (seçilen dosyayı kendi uygulamasıyla açar) | ⌘O | Ctrl+O |
| **Show in Finder / Files** (seçilen dosyayı ya da kopyayı gösterir) | ⌥⌘R | Ctrl+Alt+R |
| **Copy path** (diskte nerede olduğunu kopyalar) | ⌥⌘C | Ctrl+Alt+C |
| **Open the copy in your editor** (kopyayı editörünüzde açar; görevin ⋮ menüsünde) | ⇧⌘O | Ctrl+Shift+O |

Preview'ın göstermediği bir dosyada (bir PDF, bir film, bir sunum)
*Open with its app* ve *Show in Finder* seçenekleri sunulur. Yalnızca bir
kısmını gösterdiği Word ya da Excel dosyalarının üstünde de *Open with its
app* vardır.

### Open with its app {#open-with-its-app}

Belgeleri, resimleri, sesleri ve videoları bilgisayarınızın onlar için
kullandığı uygulamayla açar: PDF, Word, Excel, PowerPoint, OpenDocument,
CSV, düz metin ve yaygın resim, ses ve video türleri. Açılan, dosyanın
kontrolden önce yapılan Kule'ye ait kopyasıdır: uygulamada yaptığınız
değişiklikler görevin değil, o kopyanın içinde kalır.

:::warning Kule'nin açmayacakları

Bir program, bir betik (script), bir web sayfası ya da SVG, bir bağlantı
dosyası veya makro içeren bir Office dosyası asla açılmaz. Bunları ajan
yazdı; açılırlarsa bilgisayarınızda, sandbox'ın dışında çalışırlar. Bir
dosya ayrıca adının söylediği şey olmalıdır: `notes.pdf` adlı bir betik
açılmaz. Word, Excel, PowerPoint, OpenDocument ve CSV dosyaları kendi
içinde kalmalıdır: makro, denetim ya da gömülü nesne yok; bir web sayfası ya
da e-posta adresi dışında dosyanın dışına bağlantı yok; bir şey getiren ya
da çalıştıran alan veya formül yok. Bir PDF'te betik, başlatılacak bir
program, kendini gönderen bir form, başka bir belge ya da ekli dosya, ya da
açılırken çalışan bir şey olmamalıdır; şifreli bir PDF açılmaz. Ayrıca ajan
çalışırken bu işlem bekler.

:::

### Show in Finder / Files {#show-in-finder--files}

Dosyayı kendi klasöründe gösterir ve onu hiçbir zaman açmaz; böylece bir
uygulama gibi adlandırılmış bir klasör başlatılmaz.

### Kopyayı editörünüzde açmak {#open-the-copy-in-your-editor}

VS Code, Cursor ya da Zed; bilgisayarınızda hangileri varsa. Menü her
birini listeler, en son kullandığınız en üstte olur. Editör sandbox'ın
dışında, sizin kimliğinizle çalışır.

- Görevde bir ajan çalışırken ve görev orada kendiliğinden çalışacak bir
  şeyi (git config'i ya da hook'ları, bir sürüm dosyasını, bir
  `package.json`'ın adını verdiği paket yöneticisini, bir editörün
  ayarlarını ve görevlerini) değiştirdiği sürece, *Open in terminal app*'te
  olduğu gibi reddedilir.
- Kopyada commit edilmemiş değişiklikler varken Kule değişen dosyaları
  listeler ve her seferinde önce sorar. Bir editör bir klasörden
  kendiliğinden bazı şeyler çalıştırır (task'lar, ayarlar, önerilen
  eklentiler, git hook'ları); bu yüzden elinizden gelirse önce
  değişikliklere Review'da bakın ve editörün onları çalıştırmasını
  engelleyin:
  - **VS Code**'da, yazarlara güvenip güvenmediğinizi sorduğunda *no* deyin
    (Restricted Mode);
  - **Cursor**, ayarlarında Workspace Trust açık değilse sormaz.
- Kule kopyayı o editörde açtığını hatırlar. Editörün kapandığını
  göremediği için görevdeki bir sonraki çalıştırmadan önce onu kapatmanızı
  ister. Bkz.
  [Terminaller ve sunucular](terminals-and-servers.md#outside-the-sandbox)
  ve [`kule closed`](../cli/closed.md).

Editörde değiştirdikleriniz, ajanın işi gibi Review'da görünür.

## Ajana bir dosyadaki bir yeri sormak {#asking-the-agent-about-a-place-in-a-file}

Preview'da, kastettiğiniz yeri tarif etmek yerine onu gösterin:

- **biraz metin seçin**: kod satırları, bir paragraf, bir Markdown bölümü;
- bir tabloda **hücrelerin üzerinden sürükleyin**;
- bir resmin bir **bölgesinin üzerinden sürükleyin**.

Seçtiğiniz şeyin yanında bir **Ask** düğmesi görünür (Mod+Shift+A da aynı
şeyi yapar). Dosyanın tamamını sormak için Preview'ın üstündeki *Ask about
it*'i ya da hiçbir şey seçili değilken Mod+Shift+A'yı kullanın. Açılan
kutu iki seçenek sunar:

| Seçenek | Ne olur |
| --- | --- |
| **Ask now** | Soruyu yazın; ajan onu hemen alır. |
| **Add to prompt** | O yer, prompt satırının üstünde bir etiket (chip) olur; böylece birkaç yeri (20'ye kadar) toplayıp hepsi hakkında tek bir prompt yazabilirsiniz. Etiketler, prompt gönderilene kadar kendi görevlerinde kalır. |

Ajan bu yerleri prompt'unuzun ardından numaralı bir liste olarak alır; her
biri, ajanın onu bulabileceği şekilde adlandırılır:

- bir dosyanın satırları;
- bir Markdown bölümünün kaynak satırları ve başlıkları;
- bir hücre aralığı ve içindeki değerler;
- resmin kendi piksellerinde bir bölge;
- bir Word belgesinin numaralı bir paragrafı.

Resim alabilen bir ajan, seçilen her bölgeyi ayrıca bir resim olarak da
alır (bir prompt'ta üç tane). Resim alamayan bir ajan bölgenin nerede
olduğunu alır ve Live bunu belirtir. Live, bir prompt'un yerlerini onun
üstünde etiketler olarak gösterir.

## Terminalden {#from-a-terminal}

[`kule path TASK [FILE]`](../cli/path.md), kopyanın ya da içindeki bir
dosyanın diskte nerede olduğunu yazdırır (`cd "$(kule path TASK)"`).
[`kule open TASK FILE`](../cli/open.md) ise kopyadaki bir dosyayı kendi
uygulamasıyla açar; aynı türlere ve denetimlere tabidir (`--dry-run`
dosyayı denetler ve Kule'nin kopyasının nerede olduğunu yazdırır).
