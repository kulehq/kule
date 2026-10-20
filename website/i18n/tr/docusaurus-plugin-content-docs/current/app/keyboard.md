---
title: Klavye
sidebar_label: Klavye
description: Kule'nin varsayılan klavye kısayolları, komut paleti ve kendi tuşlarınızı nasıl ayarlayacağınız.
---

# Klavye

Pencerenin yaptığı her şey bir **komuttur** ve her komutun bir kısayolu
olabilir. **Mod**, macOS'ta **⌘**, Linux ve Windows'ta **Ctrl** anlamına
gelir; böylece tek bir kısayol takımı üçüne de hizmet eder.

- **Mod+K** komut paletini açar: varsayılan olarak tuşu olmayanlar dahil,
  herhangi bir komutu adıyla ve tuşlarıyla bulun.
- **Mod+/** kısayolları listeler.
- *Settings → Shortcuts* kendi kısayollarınızı ayarlamanızı sağlar (bkz.
  [Bir kısayolu değiştirmek](#changing-a-shortcut)).

## Varsayılan kısayollar {#default-shortcuts}

### Genel {#general}

| Eylem | Tuşlar |
| --- | --- |
| Tüm komutlar (komut paleti) | Mod+K |
| Klavye kısayolları | Mod+/ |
| Settings | Mod+, |

### Görevler {#tasks}

| Eylem | Tuşlar |
| --- | --- |
| Yeni görev | Mod+N |
| Sonraki / önceki görev | Ctrl+Tab / Ctrl+Shift+Tab |
| Görevi listede yukarı / aşağı taşı | Alt+Shift+↑ / Alt+Shift+↓ (liste odaktayken Alt+↑/↓) |
| Görev eylemleri (⋮ menüsü) | Mod+. |
| Görevi yeniden adlandır | F2 |
| Görevin kopyasında bir terminal aç | Mod+T |
| Görevin kopyasını editörünüzde aç | Mod+Shift+O |
| Görevden fork al | Mod+Shift+D |
| Klasörün değişikliklerini kopyaya al | Mod+Shift+G |
| Görevi sil (iki kez basın) | Mod+Backspace |

### Paneller ve yerleşim {#panels-and-layout}

| Eylem | Tuşlar |
| --- | --- |
| Tasks / Files'ı göster | Mod+0 / Mod+Shift+E |
| Ajan sohbetini / Review'u / Timeline'ı / Preview'ı göster | Mod+1 / Mod+2 / Mod+3 / Mod+4 |
| Ajan sohbetini göster ya da gizle | Mod+Shift+J |
| Sol tarafı / alt paneli / sağ tarafı göster ya da gizle | Mod+B / Mod+J / Mod+Alt+B |
| Terminali göster ya da gizle | Ctrl+\` |
| Yeni terminal | Ctrl+Shift+\` |
| Terminalde bul | macOS'ta ⌘F, diğerlerinde Ctrl+Shift+F |

### Ajan {#the-agent}

| Eylem | Tuşlar |
| --- | --- |
| Ajana yaz (prompt'a odaklan) | Mod+L |
| Prompt'u çalıştır | Enter ya da Mod+Enter (Shift+Enter: yeni satır) |
| Çalışan ajanı durdur | Mod+Shift+K |
| Preview'da seçili olanı ajana sor | Mod+Shift+A |
| Bekleyen bağlantıyı onayla / reddet | Mod+Shift+Y / Mod+Shift+N |

### Review ve dosyalar {#review-and-files}

| Eylem | Tuşlar |
| --- | --- |
| Commit et ya da onayla ve commit et | Mod+Shift+Enter |
| Değişiklikleri at (roll back; iki kez basın) | Mod+Shift+Backspace |
| Son commit'i geri al (undo; iki kez basın) | Mod+Shift+U |
| Seçili dosyayı kendi uygulamasıyla aç | Mod+O |
| Seçili dosyayı Finder'da / Files'da göster | Mod+Alt+R |
| Seçili dosyanın diskteki yerini kopyala | Mod+Alt+C |

### Görünüm {#appearance}

| Eylem | Tuşlar |
| --- | --- |
| Metni büyüt / küçült | Mod+= / Mod+- |

### Varsayılan tuşu olmayan komutlar {#commands-with-no-default}

Bunlar komut paletindedir ve *Settings → Shortcuts* altında tuş
atanabilir:

- Yeni grup; grup politikaları; bu bilgisayardaki yalıtım; tüm ajanların
  ayrıntılarını aç ya da kapat;
- görevin kopyasını Finder'da / Files'da göster; ana alanı göster ya da
  gizle; yerleşimi sıfırla;
- görevin bütçesi, grubunun bütçesi ve sınırları, ajanın sizi ne kadar
  bekleyeceği;
- bekleyen bağlantıyı onayla ve hatırla;
- balanced / careful politikayı kullan;
- profilinizi dışa / içe aktar; kısayollarda ara;
- sonraki tema, kullanılan temadan yeni tema, bir temayı içe / dışa aktar,
  hareketi aç ya da kapat, seçilen metni kopyalamayı aç ya da kapat,
  Kule'nin yazı tiplerini ve boyutlarını yeniden kullan;
- terminalin sonraki, önceki, temizle ve kapat komutları.

## Kule'nin asla almadığı tuşlar {#keys-kule-never-takes}

Sistemin ya da metin düzenlemenin kullandığı kombinasyonlar, ne ayarlarsanız
ayarlayın, hiçbir zaman alınmaz:

| Sistem | Ayrılmış |
| --- | --- |
| macOS | ⌘Q, ⌘W, ⌘H, ⌘M, ⌘C, ⌘V, ⌘X, ⌘A, ⌘Z, ⌘⇧Z, ⌘Tab, ⌘Space, Ctrl+Space, ⌘⌥Esc |
| Linux | Ctrl+C, Ctrl+V, Ctrl+X, Ctrl+A, Ctrl+Z, Ctrl+Shift+Z, Ctrl+Y, Alt+F4, Alt+Tab, Alt+Space, Ctrl+Alt+Delete, Ctrl+Alt+ok tuşları, F11 |
| Windows | Ctrl+C, Ctrl+V, Ctrl+X, Ctrl+A, Ctrl+Z, Ctrl+Y, Alt+F4, Alt+Tab, Alt+Space, Ctrl+Esc, Ctrl+Alt+Delete, F11 |

Bir metin alanında yalnızca o alan için tasarlanmış kısayollar çalışır;
böylece yazdığınız şey hiçbir zaman ele geçirilmez.

## Bir kısayolu değiştirmek {#changing-a-shortcut}

1. *Settings → Shortcuts*'u açın (ya da Mod+K'den *Search the shortcuts*'u
   çalıştırın).
2. Ada ya da tuşlara göre arayın, örneğin "theme" ya da "⌘⇧".
3. Bir komutun tuşlarına tıklayın ve istediğiniz tuşlara basın:
   - **Escape** iptal eder;
   - **Backspace** ya da **Delete** komutu kısayolsuz bırakır;
   - sistemin ya da metin düzenlemenin kullandığı bir kombinasyon
     reddedilir; yalnızca bir karakter yazacak bir kombinasyon da
     reddedilir (macOS'ta ⌘, ⌃ ya da ⌥, diğerlerinde Ctrl ya da Alt ekleyin
     veya bir fonksiyon tuşu kullanın);
   - başka bir komutun sahip olduğu bir kombinasyon bu komuta taşınır ve
     Kule hangisinden taşındığını söyler.
4. **Put back every default**, Kule'nin kendi kısayollarına döner.

Kısayollarınız Kule'nin ev dizinindeki `shortcuts.json` dosyasında saklanır
ve yeniden başlatmalardan sonra da korunur. Bir klavye düzeninde ayarlanan
bir kısayol başka bir düzende de çalışmaya devam eder, çünkü Kule tuşun
nerede olduğunu da okur.

Kısayollarınızı başka bir bilgisayara götürmek için onları bir profille dışa
aktarın (*Settings → Profile*, bkz. [Settings](settings.md)).

Menü çubuğu, ayarladığınız tuşları her öğenin yanında gösterir.
