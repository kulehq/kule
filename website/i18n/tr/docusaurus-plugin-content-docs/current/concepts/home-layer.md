---
title: Ev katmanı
sidebar_label: Ev katmanı
description: Bir görevin ev dizininize yaptığı yazmalar (~/.zshrc, ~/.config, git config --global) ev dizininize ulaşmak yerine nasıl incelemeye alınır.
---

# Ev katmanı (home layer)

## Çözdüğü sorun

Ajanlar ve çalıştırdıkları araçlar sık sık ev dizininize yazar:
`git config --global`, `~/.zshrc`'ye eklenen bir satır, `~/.config`
altında bir aracın ayarları. Bunları reddetmek araçları bozar. Geçirmek
ise bir ajanın, her kabuk açtığınızda neyin çalışacağını değiştirebilmesi
demektir.

Kule ikisini de yapmaz. Yazmalar **yakalanır**: görevin **ev katmanına**
düşer ve uygulamanızı ya da atmanızı bekler.

## Nasıl çalışır

Bir görevin komutları `HOME`'u gerçek ev dizininiz olarak değil, görevin
ev katmanı (`<task>/home`) olarak görür:

- ev dizininizin en üstündeki küçük dosyalar ve `~/.config` orada
  **klon**dur (APFS bunları tek seferde klonlar; başka yerlerde `.config`,
  64 MiB'ın altında kaldığı sürece kopyalanır);
- ev dizininizin en üstündeki yeni dosyalar da oraya düşer;
- diğer her kayıt sizin dosyanıza bir **bağlantıdır** (link), böylece araç
  zincirleri, önbellekler ve ayarlar her zamanki yerlerinde bulunur. Bir
  bağlantı üzerinden yazma, önceden olduğu gibi sandbox'a takılır;
- ajanın kendi durumu da **klonlanır**: ajan oraya yazar; yazdıkları ev
  dizininize hiç ulaşmaz ve asla incelemeye sunulmaz;
- gizli yollar (`~/.ssh`, `~/.aws`, …) katmanda hiç yer almaz;
- kabuk geçmişleri ve tamamlama (completion) dökümleri de sunulmaz.

Kabukların ve araçların başlarken okuduğu dosyaların (`.zshrc`, `.profile`,
`.gitconfig`, …) ve env dosyalarının (`~/.env`) klonlarında, gizli bir değer
gibi görünen bir ayar (`export GITHUB_TOKEN=…`) **maskelenir**: komutlar bir
yer tutucu görür (bkz. [Gizli değerler](secrets.md)), uygulama (apply) ise
sizin değerinizi geri koyar.

## Neyin beklediğini görmek

`kule status` ve uygulamadaki Review bunları listeler:

```text
in your home, caught for review (1; `kule home 1a2b3c4d apply PATH` takes one there):
  ~ ~/.zshrc  (runs every time a shell or tool starts)
```

Kabukların ya da araçların başlarken okuyup çalıştırdığı bir dosya
(`.zshrc`, `.profile`, `.gitconfig`, `~/.config/fish`,
`~/.config/autostart`, systemd kullanıcı birimleri, …) **runs at every
start** (her başlangıçta çalışır) olarak işaretlenir. Bunlara dikkatle
bakın.

## Karar vermek

```sh
kule home TASK                 # neyin beklediğini listele
kule home TASK diff ~/.zshrc   # değişikliğin dosyanıza ne yapacağı, satır satır
kule home TASK apply ~/.zshrc  # bu tek değişikliği gerçek ev dizininize al
kule home TASK discard --all   # hepsini at
kule home TASK undo            # son apply'ı geri al
```

- **apply**, dosyanızın katman oluşturulduğundaki hâliyle hâlâ aynı
  olduğunu denetler ve yerine koyduğunu görevin `home-kept/` klasöründe
  saklar.
- **undo**, o yolları o zamandan beri değiştirmediyseniz son apply'ı geri
  alır; değişiklikler yeniden incelemeyi bekler.
- Bir ev değişikliği **asla bir commit'in parçası değildir**. Ona ayrıca
  karar verirsiniz.

Uygulamada: Review → *Writes in your home*; **Show**, **Apply**,
**Discard** ve **Undo** ile. Her başlangıçta çalışan bir dosya, ilk
bakışta farkını (diff) açar.

## Çalıştırmalar arasında

Değişiklikler beklerken katman bir çalıştırmadan diğerine olduğu gibi kalır.
Hiçbiri beklemediğinde, bir sonraki çalıştırma ev dizininizin o anki
hâlinden başlar.

`kule exec` de katmanı kullanır. Bir ajan profilinin denemesi
(`kule agents try`) kullanmaz, çünkü ajanın dizinlerini neyin reddedildiğine
bakarak öğrenir.

## İlgili sayfalar

- [`kule home`](../cli/home.md)
- [Review](../app/review.md)
- [Sandbox](sandbox.md)
