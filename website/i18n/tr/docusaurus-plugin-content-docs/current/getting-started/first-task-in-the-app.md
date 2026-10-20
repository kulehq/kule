---
title: Uygulamada ilk göreviniz
sidebar_label: İlk görev (uygulama)
description: Masaüstü uygulamasında, ilk açılış ekranından bir isteme (prompt), bir soruya, Review'a, Commit'e ve Undo'ya adım adım bir yolculuk.
---

# Uygulamada ilk göreviniz

Bu sayfa, masaüstü uygulamasında tek bir görevi ilk açılıştan bir commit'e
ve onun geri alınmasına kadar adım adım anlatır. Yaklaşık on dakika sürer;
çoğu ajanın çalışma süresidir.

Uygulamaya ([Kurulum](install.md)) ve oturum açılmış en az bir ajana
ihtiyacınız var.

**Mod**, macOS'ta ⌘, Linux'ta Ctrl demektir.

## 1. İlk açılış ekranı

Henüz bir görev yokken pencere **A control room for your agents**
ekranıyla ve üç adımla açılır:

1. **Your agents (ajanlarınız).** Kule bu bilgisayarda hangi ajanların
   başlayabildiğini kontrol eder. Başlayamayan bir ajan için neyin
   kurulması gerektiğini söyler: Claude Code ve Codex Node.js (`npx`),
   Gemini kendi CLI'ını ister. Her birine, Kule olmadan yaptığınız gibi,
   bir terminalde bir kez oturum açın.
2. **How careful (ne kadar dikkatli).** Bütün görevler için bir politika
   seçin:
   - **Balanced**: sıradan dosyalardaki değişiklikler kendiliğinden geçer.
     Neyin kurulacağını, derleneceğini ya da çalışacağını belirleyenler
     (manifest'ler, lock dosyaları, CI dosyaları, editör ayarları), git
     geçmişinin yeniden yazılması ve toplu silmeler sizi bekler.
   - **Careful**: bir dosyadaki ya da git'teki her değişiklik sizi bekler.
     Sormadan yalnızca paket depolarına ve ajanların kendi API'lerine
     ulaşılır.

   Bunu daha sonra *Settings → Policy*'den değiştirebilirsiniz.
3. **A folder (bir klasör).** **Choose a folder…**'a tıklayın ve ilk
   görevin üzerinde çalışacağı klasörü seçin. Başlangıç için küçük bir test
   klasörü iyi bir seçimdir.

:::tip Ajanlar bulunamıyor mu?

Finder'dan ya da bir başlatıcıdan açıldığında uygulama `PATH`'i giriş
kabuğunuzdan (login shell) okur; böylece ajanlar `npx` ve `node`'u
terminaldeki gibi bulur. Bir ajan hâlâ eksik görünüyorsa
[Sorun giderme](../troubleshooting.md) sayfasına bakın.

:::

## 2. Görev

Kule bir **görev** açar: klasörün copy-on-write bir kopyası. Pencere artık
şunları gösterir:

- solda **Tasks**: klasörünün adını taşıyan göreviniz;
- üstte **görev başlığı**: görevin adı, klasörü ve durumu;
- ana alanda **Review**, **Timeline** ve **Preview**;
- altta istem satırıyla birlikte **Agent** paneli.

[Pencere](../app/window.md) sayfası her bölgeyi ve paneli anlatır.

## 3. Bir istem (prompt) yazın

Agent panelinin altındaki istem satırına tıklayın (ya da **Mod+L**'ye
basın) ve ajanın ne yapması gerektiğini yazın, örneğin:

```text
Read notes.md and add a "Later" section with three ideas.
```

İstemin altında:

- **agent** seçicisi istemi kimin alacağını seçer;
- **waits**, bir sorunun yanıtınızı ne kadar bekleyeceğini belirler (1
  dakikadan 8 saate kadar; varsayılan 15 dakika).

Başlatmak için **Enter**'a basın (ya da **Run**'a tıklayın). Shift+Enter
yeni bir satır açar.

## 4. Çalışmasını izleyin: Live

Agent paneli çalıştırmayı olduğu anda gösterir:

- isteminiz **You** altında, ajanın yanıtı onun adının altında;
- her **araç çağrısı** tek satırda: çalışırken `·`, bitince `✓`,
  başarısız olunca `✗`; okun ardından da değiştirdiği dosyalar;
- her komutun yazdıkları, kendi satırının altında akarak;
- istemin altında, çalıştırmanın bilgisayardan ne kadar kullandığını
  gösteren bir çubuk (`CPU 140% / 200% · 1.2 GB / 4 GB · …`).

**Stop** (Mod+Shift+K) çalıştırmayı istediğiniz an durdurur. O ana kadar
değiştirdikleri, incelemeniz için kopyada kalır.

Bkz. [Live](../app/live.md).

## 5. Bir soruyu yanıtlayın

Ajanın programı, politikanın kendiliğinden geçirmediği bir sunucuya
bağlanırsa bağlantı **bekler**. Live'da bir kart görünür ve görev, görev
listesinde **Needs you** (sizi bekleyenler) altına taşınır. Kart sunucunun
adını, portu, bekletmeye yol açan kuralı ve bir geri sayımı gösterir.

- **Approve** (Mod+Shift+Y): bu bağlantıya izin verir.
- **Approve and remember**: bundan sonra bu klasörde bu sunucuya izin
  verir.
- **Deny** (Mod+Shift+N): reddeder. Ajan onsuz devam eder.

Yanıtlanmazsa bağlantı, geri sayım bitince reddedilir. Bkz.
[Ağ ve sorular](../concepts/network-and-questions.md).

## 6. İnceleyin

Çalıştırma bitince Live **Done** der. **Review**'u açın (Mod+2):

- üstte **karar** (ruling): her şey geçiyor, bir şey sizi bekliyor ya da
  bir şey reddedildi; ve bunu hangi kuralın söylediği;
- oluşturulan, değiştirilen, silinen ya da taşınan **dosyalar** ve her
  dosyanın içinde neyin değiştiği, adıyla: bir fonksiyon, bir Markdown
  bölümü, bir JSON anahtarı. Değişen bir resim öncesini ve sonrasını
  gösterir;
- *Also commit these*'i işaretlemediğiniz sürece kopyada kalan **yok
  sayılan yollar** (`.gitignore`'un dışarıda bıraktıkları);
- ajan `~/.zshrc` ya da benzerini değiştirdiyse **ev dizininize
  yazılanlar**. Bunlar commit'ten ayrı incelenir. Bkz.
  [Ev katmanı](../concepts/home-layer.md).

Bir dosyayı kopyadaki şu anki hâliyle **Preview**'da (Mod+4) açmak için
**Files**'ta ona tıklayın.

## 7. Commit edin ya da değişiklikleri atın

- **Commit** (Mod+Shift+Enter) değişiklikleri klasörünüze uygular ve bir
  kontrol noktası saklar. Bir şey sizi bekliyorsa düğme **Approve and
  commit** olur. Reddedilen değişiklikler, onaylasanız bile dışarıda
  tutulur.
- **Roll back** (Mod+Shift+Backspace, iki kez basın) değişiklikleri atar.

Tek bir çalıştırmadan sonra karar vermek zorunda değilsiniz. Ajanı yeniden
çalıştırın: siz commit edene ya da değişiklikleri atana kadar değişiklikler
kopyada birikir.

## 8. Geri alın

**Timeline**'ı açın (Mod+3). En yeni commit'in yanında **Undo**
(Mod+Shift+U, iki kez basın) vardır. Commit'i klasörünüzden geri alır.

Timeline ayrıca her **adımı** (bir şeyi değiştiren her araç çağrısını)
listeler. **Go back here** kopyayı bir adımdan sonraki hâline getirir,
**Fork from here** ise o noktadan yeni bir görev başlatır. Bkz.
[Timeline](../app/timeline.md).

## Bitti

```text
make a task → write a prompt → watch → answer questions → review → commit or roll back → undo if needed
```

Yani: görev açın → istem yazın → izleyin → soruları yanıtlayın → inceleyin
→ commit edin ya da değişiklikleri atın → gerekirse geri alın.

Pencereyi kapatmak hiçbir şeyi durdurmaz: daemon çalıştırmayı sürdürür ve
pencereyi yeniden açtığınızda bu arada neler olduğunu görürsünüz.

Sırada:

- [Kule nasıl çalışır](../concepts/how-kule-works.md)
- [Ayarlar, bölüm bölüm](../app/settings.md)
- [Klavye kısayolları](../app/keyboard.md)
