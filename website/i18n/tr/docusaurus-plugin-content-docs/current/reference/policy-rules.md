---
title: Politika kuralları
sidebar_label: Politika kuralları
description: Kule'nin balanced ve careful politikalarındaki her kural, kararlarda gördüğünüz adlar ve kendi kurallarınızı nasıl yazacağınız.
---

# Politika kuralları

Bir görevin yaptığı her değişiklik ve bağlantı, klasörünüze ulaşmadan önce
bir [Cedar](https://www.cedarpolicy.com/) **politikası** tarafından
değerlendirilir. Bu sayfa Kule'nin iki yerleşik politikasındaki her kuralı,
`kule status`, `kule commit` ve ağ kaydında gördüğünüz kural adlarını ve
kendi kurallarınızı nasıl yazacağınızı anlatır. Arkasındaki fikir için bkz.
[Politika](../concepts/policy.md).

## Bir karara nasıl varılır {#how-a-verdict-is-reached}

Cedar yalnızca *allow* (izin ver) ya da *deny* (reddet) yanıtını verir; bu
yüzden Kule, bir görevin yaptığı her şey (her **etki**, effect) hakkında
ona iki soru sorar:

1. **Kendiliğinden geçebilir mi?** (eylem `Kule::Action::"auto"`)
2. Geçemiyorsa, **bir insana sorulabilir mi?** (eylem `Kule::Action::"ask"`)

İlk *evet* kararı verir. İki *hayır* reddeder.

| Karar | Gösterimi | Anlamı |
| --- | --- | --- |
| Allow | `passes on its own` | `kule commit` onu uygular. Bir bağlantıya izin verilir. |
| Ask | `waits for approval` | `kule commit --approve` ya da uygulamada *Approve and commit* gerekir. Bir bağlantı uygulamada yanıtınızı bekler; komut satırında sorulacak kimse olmadığı için reddedilir. |
| Deny | `denied` | Asla geçmez. |

Bir değişiklik kümesinin tamamı, etkilerinin en katı kararını alır.
Cedar'da bir `forbid` her zaman bir `permit`'e üstün gelir ve değerlendirme
sırasındaki bir hata reddeder.

Onay bekleyenler ya da reddedilenler, nedenlerine göre bir araya
toplanarak gösterilir. Her grup önce sade sözcüklerle nedeni, sonra
parantez içinde kural adlarını (her kuralın `@id`'si), sonra kararı söyler;
tuttuğu yollar altında sıralanır. Yalnızca türü yüzünden bekleyen dosyalar,
her tür için bir grup olur:

```text
policy: waits for approval
  decides what gets installed (manifests-and-lockfiles) · waits for approval
    Cargo.toml
  a kind of file this project has none of: sh (unfamiliar-kinds) · waits for approval
    scripts/release.sh
    scripts/setup.sh
net: example.com:443 · refused, would wait for approval; the command line asks no one (no-rule-lets-it-pass)
```

Kendine ait bir nedeni olmayan bir kural, örneğin kendi yazdığınız bir
politikadaki bir kural, `a rule of the policy` olarak gösterilir.

## Kule'nin balanced politikası {#kules-balanced-policy}

Varsayılan budur. Geçerliyken `kule policy` ile yazdırın.

| Kural | Tür | Neyle eşleşir | Etkisi |
| --- | --- | --- | --- |
| `ask-by-default` | permit `ask` | Her şey. | Bir `forbid` aksini söylemedikçe her şey bir insana sorulabilir. |
| `files` | permit `auto` | Her dosya değişikliği (`fs.*`). | Aşağıdaki bir kural tutmadıkça dosya değişiklikleri **kendiliğinden geçer**. Sonraki iki kuralla birlikte yalnızca gizli yolların dışındaki, bilinen türden dosyalar geçer. |
| `unfamiliar-kinds` | forbid `auto` | Gizli yolların dışında (onları `dot-paths` tutar), türü ne aşağıdaki listede olan ne de projede zaten bulunan bir dosya (asla bir silme ya da klasör değil). Bir dosyanın türü, küçük harfle uzantısıdır; uzantısı yoksa adının tamamıdır. Bilinen türler: kaynak kod (`rs go py rb java kt kts scala swift m mm c h cc cpp cxx hpp hh cs fs fsx vb ts tsx js jsx mjs cjs vue svelte php pl pm lua r jl dart ex exs erl hrl hs elm clj cljs cljc ml mli nim zig sol sql graphql gql proto css scss sass less html htm xml svg`), belgeler (`md mdx markdown rst txt adoc org tex`), veri (`json jsonc json5 yaml yml toml csv tsv`), resimler (`png jpg jpeg gif webp avif ico bmp pdf`), fontlar (`woff woff2 ttf otf`) ve `README`, `LICENSE`, `LICENCE`, `COPYING`, `NOTICE`, `AUTHORS`, `CONTRIBUTORS`, `CHANGELOG` adları. Projede zaten bulunan bir tür, klasörünüzün git index'inin gizli yolların dışında tuttuğu bir türdür; Kule bunu görevin kopyasından değil, her zaman klasörünüzden okur. Git dışında yalnızca liste sayılır. Betikler (`sh`, `ps1`, `bat`, …) bilerek dışarıda bırakılmıştır. | **Onay bekler.** Türüne göre hatırlanabilir. |
| `dot-paths` | forbid `auto` | Yolunun herhangi bir parçası noktayla başlayan bir dosya (asla bir silme ya da klasör değil): `.env`, `.config/tool.json`, `src/.eslintrc`; türü ne olursa olsun, izleniyor olsun ya da olmasın. Araçlar ayarlarını orada arar. | **Onay bekler.** Yoluna göre hatırlanabilir. |
| `manifests-and-lockfiles` | forbid `auto` | Neyin kurulacağını ya da derleneceğini belirleyen dosyalar; adlarına göre, herhangi bir klasörde, büyük/küçük harf fark etmeksizin: `package.json`, `package-lock.json`, `npm-shrinkwrap.json`, `yarn.lock`, `pnpm-lock.yaml`, `bun.lock(b)`, `Cargo.toml`, `Cargo.lock`, `Directory.Packages.props`, `Directory.Build.props`/`.targets`, `nuget.config`, `packages.lock.json`, `global.json`, `go.mod`, `go.sum`, `pyproject.toml`, `setup.py`, `setup.cfg`, `poetry.lock`, `uv.lock`, `Pipfile`(`.lock`), `Gemfile`(`.lock`), Gradle derleme ve ayar dosyaları, `pom.xml`. | **Onay bekler.** Hatırlanabilir. |
| `project-files` | forbid `auto` | `*.csproj`, `*.fsproj`, `requirements*.txt`. | **Onay bekler.** Hatırlanabilir. |
| `automation` | forbid `auto` | Kimse projeyi çalıştırmadan kod çalıştıran ya da bir ajanı yönlendiren şeyler. `.github`, `.circleci`, `.devcontainer`, `.vscode`, `.idea`, `.zed`, `.kule`, `.husky`, `.claude`, `.cursor`, `.gemini`, `.codex`, `.cargo`, `.mvn`, `.yarn`, `.buildkite`, `.gitea`, `.forgejo`, `.woodpecker`, `.windsurf`, `.continue`, `.roo`, `.clinerules` ya da `gradle/wrapper/` içindeki her şey; ve `.gitlab-ci*`, `.envrc`, `.gitattributes`, `.gitmodules`, `.npmrc`, `.yarnrc(.yml)`, `.pnpmfile.cjs`, `build.rs`, `gradlew`, `mise.toml`, `.tool-versions`, `.pre-commit-config.yaml`, `lefthook.yml`, `.mcp.json`, `CLAUDE.md`, `AGENTS.md`, `GEMINI.md`, `.cursorrules`, `rust-toolchain(.toml)`, `mvnw`, `bunfig.toml`, `dotnet-tools.json`, `azure-pipelines.yml`, `.travis.yml`, `Jenkinsfile`, `bitbucket-pipelines.yml`, `opencode.json`, `.aider.conf.yml` gibi dosyalar; bir kabuk isteminin klasöre girerken okuduğu sürüm dosyaları (`.nvmrc`, `.node-version`, `.python-version`, `.ruby-version`, `.java-version`, `.go-version`, `.sdkmanrc`, `.terraform-version`, `mise.local.toml`, `.rtx.toml`, mise'nin `config.toml`'u, `.mise` içindeki her şey) ve `*.code-workspace`. Bunlardan biri değişmiş bir kopyada Kule'nin terminali ve editörü açılmaz. | **Onay bekler.** Hatırlanamaz: kendiliğinden kod çalıştıran şey her seferinde sorulur. |
| `other-vcs` | forbid `auto` | Herhangi bir derinlikte, başka bir sürüm kontrol sisteminin deposu: `.hg`, `.sl`, `.jj`, `_darcs`, `.pijul`, `.bzr`, `.svn`, `.fossil-settings` içindeki her şey ve `.fslckout` ya da `_FOSSIL_`. Config'i ve hook'ları bir sonraki komutunda çalışır ve Kule hiçbirini bir depo olarak okumaz. Bunlardan biri değişmiş bir kopyada Kule'nin terminali ve editörü açılmaz. | **Onay bekler.** Hatırlanamaz. |
| `nested-repository` | forbid `auto` | Çalışma klasörünün içine yerleşmiş bir deponun `.git` klasöründeki dosyalar. | **Onay bekler.** |
| `nested-git-hooks` | forbid all | İç içe bir depoda oluşturulan ya da değiştirilen bir hook. | **Reddedilir.** |
| `permissions` | forbid `auto` | Çalıştırılabilir yapılan bir dosya (klasördekinde olmayan herhangi bir `x` biti, ya da biti olan yeni bir dosya), ya da herkesin yazabileceği hâle getirilen bir dosya veya klasör. Klasörün zaten çalıştırdığı bir dosya geçmeye devam eder. | **Onay bekler.** Hatırlanamaz. |
| `extended-attributes` | forbid `auto` | macOS'ta, genişletilmiş öznitelikleri klasördeki dosyanınkinden farklı olan bir dosya (eklenen ya da kaldırılan bir karantina işareti, bir `user.*` özniteliği): commit onları klasöre taşır. macOS'un her yazmada kendiliğinden koyduğu şey (`com.apple.provenance`) dışarıda bırakılır. | **Onay bekler.** Hatırlanamaz. |
| `links` | forbid `auto` | Oluşturulan, değiştirilen ya da silinen bir sembolik bağlantı. | **Onay bekler.** Bir bağlantı, gösterdiği her yere götürür. |
| `mass-delete` | forbid `auto` | Tek bir değişiklik kümesinde 200'den fazla silme. Bir taşıma silme sayılmaz. | **Onay bekler.** |
| `deletes-outside-git` | forbid `auto` | Git deposu olmayan bir klasörde 20'den fazla silme. | **Onay bekler.** Onları yalnızca görevin kendi undo'su geri getirebilir. |
| `branch-forward` | permit `auto` | İleri taşınan (fast-forward) bir dal (`refs/heads/*`). | **Kendiliğinden geçer.** |
| `branch-created` | permit `auto` | Yeni bir dal. | **Kendiliğinden geçer.** Yeni bir etiket (tag), not ya da replacement onay bekler. |
| `package-registries` | permit `auto` | `registry.npmjs.org`, `registry.yarnpkg.com`, `crates.io`, `index.crates.io`, `static.crates.io`, `static.rust-lang.org`, `api.nuget.org`, `globalcdn.nuget.org`, `pypi.org`, `files.pythonhosted.org`, `rubygems.org`, `repo.maven.apache.org`, `repo1.maven.org` adreslerine HTTPS (443 numaralı port). | **Kendiliğinden geçer.** |
| `git-hooks` | forbid all | Oluşturulan ya da değiştirilen bir git hook'u. Hook'lar bir sonraki git komutunuzda, sizin yetkilerinizle çalışır. | **Reddedilir.** |

Hiçbir `auto` permit ile eşleşmeyen ve bu yüzden **onay bekleyenler**
(`no-rule-lets-it-pass`): git config değişiklikleri (`.git` içinde git'in
veri olarak tutmadığı her şey: nesneler, ref'ler, index, log'lar
dışındakiler), dosyalardan farklı
olan stage'lenmiş girdiler, geçmişin yeniden yazılması, silinen dallar,
değiştirilen bir `HEAD`, diğer ref'ler (etiketler, notlar, replacement'lar)
ve git host'ları dahil yukarıda listelenmeyen her host (TLS içinde bir
push, bir fetch'ten ayırt edilemez).

## Kule'nin careful politikası {#kules-careful-policy}

`kule policy careful` bu seçimi `~/.kule/policy-choice` içinde saklar. Kule
bundan sonra balanced politikayı, sonuna bir kural daha eklenmiş olarak
uygular:

| Kural | Tür | Neyle eşleşir | Etkisi |
| --- | --- | --- | --- |
| `careful` | forbid `auto` | Her dosya değişikliği ve her git değişikliği. | **Her değişiklik sizi bekler.** Yalnızca yukarıdaki bağlantılar (ve her ajanın kendi host'ları) kendiliğinden geçer. |

Careful altında hatırlanan bir dosya geçmez: `careful` her dosyayı sorar ve
bu soru kalıcı olarak yanıtlanamaz.

Saklanan metin değil seçimdir; bu yüzden careful her zaman bu sürümün
kurallarını ifade eder. Önceki bir sürümün yazdığı (0.2.0-preview.1 ile
0.2.0-preview.11 arası) bir balanced ya da careful metnini tutan
`~/.kule/policy.cedar`, kendi politikanız değil, o seçim sayılır: bugünün
kurallarıyla değerlendirilir ve `kule policy balanced` ya da `careful` onun
yerini alır.

## Kule'nin eklediği adlar {#names-kule-adds}

Bunlar politika metninde görünmez; Kule onları değerlendirme sırasında
ekler.

| Ad | Nerede | Anlamı |
| --- | --- | --- |
| `no-rule-lets-it-pass` | onay bekler | Hiçbir `auto` permit eşleşmedi. |
| `no-rule-allows-asking` | reddedilir | Hiçbir `ask` permit eşleşmedi; bu yüzden size sorulamaz bile. `ask-by-default`'u dışarıda bırakan kendi politikanızla olur. |
| `policy-error: …` | reddedilir | Cedar bir kuralı değerlendiremedi; reddeder. |
| `remembered` | kendiliğinden geçer | Kule'ye bir daha sormamasını söylediğiniz bir dosya ya da dosya türü ([`kule rules`](../cli/rules.md)). |
| `repo:<id>` | kuralın dediği gibi | Deponun kendi `.kule/policy.cedar` dosyasından bir kural. `@id`'si ne olursa olsun, hatırladığınız hiçbir yanıt onu iptal etmez. |
| `remembered-<id>` | kendiliğinden geçer | Kule'ye bir daha sormamasını söylediğiniz bir bağlantı. |
| `profile-<agent>` | kendiliğinden geçer | Ajanın profilindeki bir host ya da modelinin API'si; **yalnızca o ajan çalışırken** açıktır (örneğin `api.anthropic.com` için `profile-claude-code`). |

## Bir kuralın bakabildikleri {#what-a-rule-can-look-at}

Her etki, şu özniteliklere sahip bir `Kule::Effect` varlığıdır:

| Öznitelik | Değer |
| --- | --- |
| `kind` | `fs.create`, `fs.modify`, `fs.delete`, `git.ref`, `git.config`, `git.hook`, `git.staged`, `git.other`, `net.connect` |
| `path`, `name` | Çalışma klasöründeki yol ve son parçası (`git.ref` için `""`). |
| `folded_path`, `folded_name` | Aynıları, küçük harfle. Bunlarla eşleştirin: macOS diskleri varsayılan olarak büyük/küçük harfi yok sayar, onları okuyan araçlar da öyle. |
| `parts` | Küçük harfe çevrilmiş yolun parçaları; bir klasörü herhangi bir yerde eşleştirmek için (`resource.parts.contains("migrations")`). |
| `entry` | `fs.*` için `file`, `dir` ya da `symlink`; diğerlerinde `""`. |
| `change` | `created`, `modified` ya da `deleted`. |
| `ref` | `git.ref` için: `refs/heads/main` gibi bir referans. |
| `fast_forward` | `git.ref` için: yeni değer eskisini geçmişinde tutuyor mu. |
| `commits` | `git.ref` için: kaç commit eklendiği. |
| `host`, `port` | `net.connect` için. |
| `gains` | `fs.*` için: girdinin izin bitlerinde klasördekinde olmayan, kazandığı şey, `executable` ve `world-writable` (`!resource.gains.isEmpty()`). |
| `xattrs_changed` | `fs.*` için: dosyanın genişletilmiş öznitelikleri klasördekinden farklı (macOS). |
| `file_kind` | `fs.*` için: adın küçük harfle uzantısı (`vue`), uzantısı yoksa adın tamamı (`makefile`). |
| `kind_tracked` | `fs.*` için: klasörün git index'i, gizli yolların dışında bu türden bir dosya tutuyor mu. Görevin kopyasından değil, klasörünüzden okunur. |
| `dotted` | `fs.*` için: yolun bir parçası noktayla başlıyor mu, ya da yol bir deponun kendi dosyalarının içinde mi. |

Ve `context`:

| Context | Değer |
| --- | --- |
| `deletes` | Bu değişiklik kümesinde silinen yollar (bir taşıma silme sayılmaz). |
| `files` | Değişen yollar. |
| `repository` | Çalışma klasörünün bir git deposu olup olmadığı. |

## Kendi politikanız, her görev için {#your-own-policy-for-every-task}

Bir politikayı Kule'nin ev dizinine `policy.cedar` olarak kaydedin
(`~/.kule/policy.cedar`). Kule oraya kendisi hiçbir şey yazmaz: bu dosya
yalnızca sizin kendi politikanızdır. Her görev için Kule'ninkinin **yerine
geçer**
(kendi politikası olan bir grup kendisininkini korur). Bir `forbid` hiçbir
zaman geçersiz kılınamadığı için Kule'nin kurallarını gevşetmenin tek yolu
onları değiştirmektir; bu yüzden Kule'nin metninden başlayın ve yalnızca
değiştirmek istediğiniz yeri değiştirin:

```sh
kule policy > ~/my-policy.cedar      # balanced geçerliyken
# düzenleyin, sonra:
cp ~/my-policy.cedar ~/.kule/policy.cedar
kule policy | tail -5                # Kule artık sizinkini yazar
```

Örnek: bir şirket paket aynasının (mirror) sormadan geçmesine izin verin.
Kopyanın sonuna ekleyin:

```cedar
// Our own: the company's package mirror passes on its own.
@id("company-mirror")
permit (principal, action == Kule::Action::"auto", resource)
when { resource.kind == "net.connect" && resource.port == 443 && resource.host == "npm.example.com" };
```

Oraya giden bir bağlantı sonra şöyle görünür:

```text
net: npm.example.com:443 · tunneled (company-mirror)
```

- Kule dosyanızın üzerine asla yazmaz: dosya oradayken `kule policy
  balanced` ve `careful` reddeder. Geri dönmek için dosyayı silin ya da
  taşıyın.
- Ayrıştırılamayan bir dosya Kule'nin değerlendirme yapmasını durdurur; bu
  yüzden komutlar, çalıştırmalar ve commit'ler, dosya düzeltilene kadar
  reddedilir (`kule: policy: unexpected end of input`).
- `ask-by-default`'u tutun; yoksa size hiçbir şey sorulamaz ve bekletilen
  her değişiklik reddedilir.

## Bir deponun kendi kuralları {#a-repositorys-own-rules}

Bir depo, kökündeki `.kule/policy.cedar` ile üzerinde çalışan herkes için
politikayı sıkılaştırabilir. Bu dosya **yalnızca `forbid` kuralları**
içerebilir:

```cedar
// Changes to database migrations never pass, not even with approval.
@id("no-migrations")
forbid (principal, action, resource)
when { resource.kind like "fs.*" && resource.parts.contains("migrations") };
```

Sonra:

```console
$ kule status 1046fb30
4 file changes: +4 ~0 -0
  + db/
  + db/migrations/
  + db/migrations/001.sql
  + notes.md
policy: denied
  the repository's own rule (repo:no-migrations) · denied
    db/migrations
    db/migrations/001.sql
$ kule commit 1046fb30 --approve
...
kule: not committed; the policy denies the changes above
```

`forbid (principal, action, resource)` iki soruyu da kapsar; bu yüzden
değişiklik **reddedilir**. Onun yerine onay **beklemesi** için
`action == Kule::Action::"auto"` yazın.

- Kule bu dosyayı bir görevin kopyasından değil, her zaman **gerçek
  klasörünüzden** okur; böylece bir ajan onu zayıflatamaz. Bir kopyada ona
  yapılan değişiklikler sizi bekler (`automation`, `dot-paths`) ve
  hatırlanamaz.
- İçindeki bir `permit` reddedilir: `.kule/policy.cedar may only contain
  forbid rules: a repository can tighten the policy, never loosen it`.
- Deponun forbid kuralları her politikada geçerlidir: balanced, careful,
  sizinki ve her grubunki.

## Bir grubun kendi politikası {#a-groups-own-policy}

`kule group policy ID own --file PATH`, bir gruba kaydedilmeden önce
denetlenen kendi politikasını verir. `policy.cedar` için geçerli olan
kurallar aynen geçerlidir. Bkz. [`kule group`](../cli/group.md).

## Ayrıca bakın {#see-also}

- [`kule policy`](../cli/policy.md), [`kule rules`](../cli/rules.md), [`kule commit`](../cli/commit.md)
- [Politika](../concepts/policy.md)
- [Cedar'ın belgeleri](https://docs.cedarpolicy.com/)
