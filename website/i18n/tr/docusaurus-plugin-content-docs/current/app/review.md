---
title: İnceleme ve commit
sidebar_label: Review
description: Bir görevin neyi değiştirdiğini ve politikanın bu konuda ne dediğini görün, sonra klasörünüze commit edin ya da değişiklikleri atın.
---

# İnceleme ve commit

**Review** (Mod+2), görevin henüz commit edilmemiş değişikliklerini ve
[politikanın](../concepts/policy.md) bunlar hakkındaki kararını gösterir.
Burada (ya da [`kule commit`](../cli/commit.md) ile) commit edene kadar
gerçek klasörünüze hiçbir şey ulaşmaz.

## Karar {#the-ruling}

En üstte karar üç şeyden birini ve bunu hangi kuralın söylediğini belirtir:

- **All pass (hepsi geçer)**: her değişiklik klasöre kendiliğinden
  ulaşabilir.
- **Some wait for your approval (bazıları onayınızı bekler)**: örneğin
  balanced politikada değişmiş bir `package.json`, lockfile ya da CI
  dosyası.
- **Some are denied (bazıları reddedilir)**: örneğin yeni bir git hook'u.
  Reddedilen değişiklikler, siz onaylasanız bile klasöre hiçbir zaman
  ulaşmaz.

## Neler değişti {#what-changed}

- **Oluşturulan, değiştirilen ve silinen dosyalar.** Taşımalar silmelerden
  ayrı gösterilir (`» photos/ → 2026/`).
- **Her dosyanın içinde** Review, neyin değiştiğini bir insanın kullandığı
  adlarla söyler: Rust, C#, TypeScript, JavaScript, Python ve Go'da
  fonksiyonlar, metotlar, alanlar ve tipler; Markdown'da bölümler; JSON ve
  TOML'da anahtarlar. Bir dosyadaki her değişiklik bunlardan birinin ya da
  dosyanın en üst düzeyinin (top level) altında listelenir:

  ```
    ~ notes.md
        + section Plan › Later
    ~ src/task.rs
        ~ method Task::commit
        + method Task::check
  ```

- **Resimler** (PNG, JPEG, GIF, WebP) neydi ve ne olduğunu gösterir.
  Pencereye yalnızca başlığı (header) gerçekten böyle bir resim olduğunu
  kanıtlayan, 8 MiB ve 40 megapikselin altındaki dosyalar ulaşır.
- Klasör bir depo (repository) olduğunda **git** değişiklikleri: dal
  (branch) hareketleri, yeni nesneler, index ve hook'lar ile `config` için
  ayrı satırlar; çünkü bunlar bir sonraki `git` komutunun kod çalıştırmasına
  yol açar. Bkz. [Git](../concepts/git.md).

## Yok sayılan yollar {#ignored-paths}

Klasörün `.gitignore` dosyasının dışarıda bıraktığı dosyalar (derleme
çıktıları, önbellekler, `.env`) kopyada kalır. Review bunları listeler;
onları da almak için **Also commit these** kutusunu işaretleyin. Politika
bunları da değerlendirir. Kurallar her zaman sizin klasörünüzden gelir,
ajanın düzenlediği kopyadan asla gelmez.

## Ev dizininize yazılanlar {#writes-in-your-home}

`~/.zshrc`'yi değiştiren, `git config --global` çalıştıran ya da
`~/.config` altına yazan bir ajan ev dizininize ulaşmadı: yazdıkları,
görevin ev katmanında (home layer) yakalandı (bkz.
[Ev katmanı](../concepts/home-layer.md)). Commit'ten ayrı olarak burada
beklerler:

| Düğme | Ne yapar |
| --- | --- |
| **Show** | Değişikliğin dosyanıza ne yapacağını satır satır gösteren bir diff; gizli değere benzeyen her şey iki tarafta da `‹hidden …›` olarak görünür. |
| **Apply** | Dosyanızın hâlâ eskisi gibi olduğunu denetledikten sonra bu tek değişikliği gerçek ev dizininize alır. Yerine geçtiği içerik görevde saklanır. |
| **Discard** | Değişikliği atar. |
| **Undo** | O yolları o zamandan beri değiştirmediyseniz son apply'ı geri alır. |

Her açılışta çalışan bir dosya (`.zshrc`, `.profile`, `.gitconfig`,
otomatik başlatma kayıtları, systemd kullanıcı birimleri) *runs at every
start* olarak işaretlenir ve diff'i hemen açılır. Ev dizinindeki bir
değişiklik hiçbir zaman bir commit'in parçası olmaz.

## Görevlerin buluştuğu yerler {#where-tasks-meet}

Aynı klasördeki görevler birbirlerinin işini hiçbir zaman görmez. Bu
yüzden Review, ikisi de hâlâ sürerken aynı dosyaları değiştiren diğer
görevleri metot, bölüm ya da anahtar düzeyine kadar listeler
(`task 5e6f7a8b changes them too: task.rs · method Task::commit`). İkinciler
commit ettiğinde, ikisinin de değiştirdiği bir dosya üç yönlü (three-way)
birleştirilir; yalnızca ikisinde de aynı parçanın değişmesi bir çakışmadır.
Bkz. [Senkronizasyon ve birleştirme](../concepts/sync-and-merging.md).

## Klasörde değişenler {#changed-in-the-folder}

Kopya klasörünüzü kendiliğinden izlemez: ajan, klasör üzerinde kopyanın
oluşturulduğu ya da en son güncellendiği andaki haliyle çalışır. Review,
klasörün o zamandan beri değiştirdiklerini (örneğin sizin kendi
düzenlemeleriniz ya da başka bir görevin commit'i) ve bu dosyalardan
hangilerini görevin de değiştirdiğini listeler.

**Bring into the copy** (Mod+Shift+G, görevin ⋮ menüsünde de var) bunları
kopyaya alır:

- yalnızca klasörün değiştirdiği bir dosya olduğu gibi gelir;
- görevin de değiştirdiği bir dosya metot, bölüm ya da anahtar bazında
  birleştirilir ve görevin değişikliği olarak kalır;
- bunlardan herhangi biri birleşmezse hiçbir şey gelmez ve Kule ikisinin
  neyi değiştirdiğini söyler;
- `.env` gibi ayar dosyaları, kopyanın geri kalanı gibi maskelenmiş olarak
  gelir;
- adımlar, bir commit'ten sonraki gibi baştan başlar ve ajan, bir sonraki
  sırasının başında neyin geldiğini duyar.

Bir ajan çalışırken bu işlem reddedilir. Görevin sandbox'ında hâlâ çalışan
programlar, bir commit öncesinde olduğu gibi önce durdurulur. Klasörünüz
yalnızca okunur.

## Düğmeler {#the-buttons}

| Düğme | Ne zaman | Tuşlar |
| --- | --- | --- |
| **Commit** | Her şey geçtiğinde. Değişiklikleri klasörünüze uygular ve bir kontrol noktası saklar. | Mod+Shift+Enter |
| **Approve and commit** | Bir şey sizi beklediğinde. Geçenleri ve onayladıklarınızı uygular; reddedilen değişiklikler geride tutulur. | Mod+Shift+Enter |
| **Don't ask again in this folder** | Bekletilen bir dosyada (bir manifest, CI ya da proje dosyası): önce neleri geçireceğini gösterir, sonra bunu bu klasör için hatırlar. | yok |
| **Roll back** | Değişiklikleri atar; kopya yeniden klasörle aynı olur. İki kez basın. | Mod+Shift+Backspace |

Bir commit'ten sonra [Timeline](timeline.md)'daki **Undo** (Mod+Shift+U, iki
kez basın), en yeni commit'i klasörünüzden geri alır.

:::note

Klasörünüz commit'in altında değiştiyse commit reddedilir: ajanın da
değiştirdiği bir dosyayı düzenlediniz ya da ajanın sildiği bir dizine dosya
eklediniz. Önce klasörün değişikliklerini kopyaya alın. Commit her dosyayı
bir yeniden adlandırma (rename) ile yazar; böylece kimse yarım bir dosya
görmez.

:::

## Terminalden {#from-a-terminal}

[`kule status`](../cli/status.md) aynı değişiklikleri ve kararı gösterir
(betikler için `--json`). [`kule commit`](../cli/commit.md) commit eder
(`--approve`, `--remember`, `--include-ignored`),
[`kule rollback`](../cli/rollback.md) değişiklikleri atar,
[`kule sync`](../cli/sync.md) klasörün değişikliklerini kopyaya alır,
[`kule home`](../cli/home.md) da ev dizininize yazılanları yönetir.
