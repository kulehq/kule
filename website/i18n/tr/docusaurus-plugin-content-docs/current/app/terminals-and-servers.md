---
title: Terminaller ve sunucular
sidebar_label: Terminaller ve sunucular
description: Bir görevin kopyasında kendi kabuğunuzu açın ve bir görevin çalıştırdığı sunuculara tarayıcınızdan erişin.
---

# Terminaller ve sunucular

## Terminaller {#terminals}

**Terminal** paneli, görevin kopyasında açtığınız kendi kabuklarınızı tutar;
istediğiniz kadar açabilirsiniz, her biri bir sekmedir.

| Eylem | Tuşlar |
| --- | --- |
| Terminali göster ya da gizle | Ctrl+\` |
| Başka bir terminal aç | Ctrl+Shift+\` |
| Görevin ⋮ menüsünden bir terminal aç (*Open terminal*) | Mod+T |
| Terminalde metin bul | macOS'ta ⌘F, diğerlerinde Ctrl+Shift+F |
| Bir http ya da https bağlantısını aç | ⌘/Ctrl+tık |
| Sonraki, önceki terminal; terminali temizle ya da kapat | varsayılan yok; *Settings → Shortcuts* altında tuş atayın |

:::warning Terminal sizindir, sandbox'ın dışındadır

Terminal, sandbox'ın **dışında**, sizin kendi izinlerinizle çalışır; panelin
köşesinde *outside the sandbox* yazar. Oradan kopyada değiştirdikleriniz,
ajanın işi gibi incelenir ve commit edilir; ama komutların kendisi sizin
erişebildiğiniz her şeye erişebilir.

:::

- Linux ve Windows'ta kabuktan yalnızca Ctrl+Shift kısayolları ve fonksiyon
  tuşları çıkar; böylece Ctrl+L ve diğerleri kabuğa ait kalır.
- Görevde bir ajan çalışırken ve görev kopyada kendiliğinden çalışacak bir
  şeyi değiştirdiği sürece Kule **terminal açmayı reddeder**: git config ya
  da hook'lar (prompt'unuzdaki `git status` bunları çalıştırırdı),
  `rust-toolchain.toml` ya da `.nvmrc` gibi bir sürüm dosyası, `.envrc` ya
  da bir editörün ayarları; hepsi sandbox'ın dışında. Önce bunları inceleyip
  commit edin ya da değişiklikleri atın. Aynı kural *Open the copy in your
  editor* için de geçerlidir. Bkz.
  [kendiliğinden çalışanlar](../concepts/git.md#what-runs-by-itself-protects-your-terminal-too).

## Görevin çalıştırdığı sunucular {#servers-the-task-runs}

Görevin kopyasındaki bir şey bir port'u dinlediğinde (ajanın
`npm run dev`'i ya da görevin terminalinde sizin başlattığınız bir sunucu),
görev başlığında `localhost:5173 ↗` gibi bir **etiket** (chip) görünür.
Sunucuyu tarayıcınızda açmak için üzerine tıklayın.

Bir görevin başlattığı sunucular yalnızca bu bilgisayarda dinler: ağınızdaki
hiçbir şey (bir kafenin Wi-Fi'ı, ofis LAN'ı) bir ajanın çalıştırdığı şeye
ulaşamaz.

- **Linux**'ta Kule, program ne olursa olsun her dinleme adresini
  loopback'e çevirir.
- **macOS**'ta:
  - Node, Python ve Ruby sunucuları, her adresi dinlemek istediklerinde
    loopback'e yönlendirilir;
  - diğer programlar (Go, Rust, Java) için Kule, çalıştırma sürdüğü sürece
    port'u **korur** (shield) ve etikette 🛡 görünür;
  - kendine ait bir ağ adresini dinleyen bir sunucu durdurulur ve Live
    nedenini söyler.

Etiketler ayrıca neyin dikkatinizi gerektirdiğini de söyler:

| Etiket | Anlamı |
| --- | --- |
| Düz, ↗ ile | Yalnızca bu bilgisayardaki bir sunucu. Açmak için tıklayın. |
| 🛡 ile (macOS) | Port'unu Kule'nin çalıştırma sürdüğü sürece ağdan koruduğu bir sunucu. |
| **Uyarılı kehribar** | Ağın hâlâ erişebildiği bir sunucu; genellikle bir terminalde kendi başlattığınız bir sunucu. Kule sizinkilere dokunmaz. |
| **Gri** (Linux) | Sandbox'ın kendi ağının içindeki, bu bilgisayarın erişemediği bir sunucu. |

Ajanın başlattığı bir sunucu, çalıştırmasıyla birlikte sona erer. Bir
terminalde sizin başlattığınız bir sunucu ise kalır.

## Terminalden {#from-a-terminal}

[`kule shell TASK`](../cli/shell.md), kabuğunuzu kopyada, sandbox'ın
dışında açar ve aynı durumlarda reddeder.
[`kule path TASK`](../cli/path.md), `cd "$(kule path TASK)"` için kopyanın
nerede olduğunu yazdırır ve orada bir şeyin görevin değiştirdiklerini
çalıştıracağı durumlarda stderr'e uyarı yazar.
