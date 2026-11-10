# SolidSketch — revue d'architecture et suivi

Revue du 2026-11-09, sur le premier embryon (≈ 9 000 lignes de Rust, 5 crates). Les décisions
d'architecture elles-mêmes sont dans [ARCHITECTURE.md](ARCHITECTURE.md) ; ce document garde
l'avis critique et la liste de ce qui reste à faire. **Tenir la todo à jour** (en bas).

## Verdict

Socle très solide pour un premier jet : découpage propre, décisions documentées, et des sujets
que la plupart des projets CAO découvrent trop tard sont déjà réglés (précision loin de
l'origine, annulation bon marché, rendu indépendant de l'UI). Les vrais risques ne sont pas dans
le code : **la dépendance au noyau SolidCore** et **quelques choix de modèle qui coûteront cher
quand le modèle grossira**. Le plus urgent — le noyau sur le thread de l'UI — est traité (§7 ter).

## Points forts

- **Découpage en crates** (`kernel` → `model` / `render` → `tools` → app) : `render` ne connaît
  ni egui ni le document, `tools` tourne sans GPU ; le cœur interactif est testé sans affichage.
- **Le record B-rep comme état**, le noyau instancié le temps d'un appel : aucun état noyau à
  garder cohérent, `Clone` / serde gratuits, `catch_unwind` sur un noyau jeté est sain.
- **Annuler par instantanés partagés** (`Arc` + `make_mut`) : simple, correct, aucune inverse.
- **Précision** : rendu relatif à la caméra en `f64`, maillages recentrés, profondeur inversée,
  testé à 100 km de l'origine.
- **Opérations typées et sérialisables** : la bonne discipline (voir la réserve sur le
  paramétrique plus bas).
- **Inférences sur la géométrie exacte du record**, pas sur le maillage : un vrai plus.

## Risques d'architecture

Par ordre d'importance.

1. **Noyau sur le thread de l'UI, paniques fatales en wasm.** — *Traité* (ARCHITECTURE §7 ter) :
   opérations en deux temps, noyau sur un thread (desktop) ou dans un Web Worker (web), relance
   du worker après un plantage.
2. **Le rendu met en buffer par définition.** Presque tous les solides vivent dans la racine :
   chaque opération sur l'un d'eux recopie et renvoie au GPU tout le buffer de la racine.
   Instancier les composants est juste ; faire de la définition l'unité de buffer, non. Piste :
   un buffer par solide, ou un allocateur de plages, avec des draws groupés. — *Décidé le
   2026-11-10* (ARCHITECTURE §2, §7 bis) : l'unité de rendu devient le B-rep final d'une pièce.
3. **Déplacer un solide réécrit son B-rep.** Coût proportionnel à la complexité (aller-retour
   noyau, remaillage), dérive numérique après des rotations répétées, cache de maillage
   invalidé. Piste à peser : une transformation propre au solide, appliquée au B-rep seulement
   au moment d'un booléen (s'écarte du modèle SketchUp, plus sûr en B-rep exact). — *Décidé
   le 2026-11-10* (ARCHITECTURE §2.2) : tout est exemplaire d'une pièce, T jamais intégré.
4. **Le format de fichier dépend du serde interne de `solidcore_record`.** Monter de version
   SolidCore peut rendre d'anciens fichiers illisibles sans prévenir. Il faut des fichiers de
   référence dans les tests, et à terme un format binaire ou compressé (le JSON de B-rep
   grossit vite).
5. **« Ouvert au paramétrique » est surévalué.** Les opérations désignent les faces par leur
   index dans le record, qui change à chaque booléen : le journal n'est pas rejouable. Le vrai
   chantier, si le paramétrique compte, est le nommage persistant.
6. **`Editor` est fourre-tout** (historique, scène, caméra, sélection, verrous, suivi de
   points, temps, et maintenant le service du noyau) et `infer.rs` fait ~1 270 lignes. À
   découper avant pousser/tirer.

Plus mineurs :

- Booléen avec un outil situé dans un composant : l'outil disparaît de la définition, donc de
  toutes ses instances. Sans doute inattendu. — *Réglé par le §2* : seul l'exemplaire outil
  devient un usinage ; les autres exemplaires de sa pièce restent.
- Historique sans plafond mémoire (chaque B-rep modifié gardé en entier) ; depuis §7 ter, le
  cache de maillages suit l'historique et grossit avec lui.
- `purge_orphan_groups` en O(définitions × entités) à chaque opération (disparaît avec les
  groupes, §2).
- Textes français dans `model` (« Modèle », « Boîte », libellés) à côté d'erreurs en anglais :
  l'internationalisation se fera dans le crate le plus bas.

## Dépendances

| Dépendance | Avis |
|---|---|
| **SolidCore** (git, rev figée) | **Risque n°1.** Source unique, trois crates hors crates.io, port de SMLib à la robustesse non prouvée (booléens, décalage). Figer la rev est juste à court terme ; ensuite un fork sous contrôle, des tests de robustesse du noyau (tangences, coplanaires, petites arêtes) et une API `kernel` étroite — elle l'est déjà. |
| **egui / eframe 0.37** | Bon choix pour un prototype natif + web en Rust pur. Limites à venir : docking (`egui_dock`), saisie riche, accessibilité, finition. |
| **wgpu 30** (imposé par egui-wgpu) | Montées de version liées à egui. Rendu hors écran + texture egui : une copie de plus, mais `render` testable. Bon compromis. |
| **glam (f64)** | Parfait. |
| **egui_extras `svg`** | Tire `resvg`/`usvg`, lourds en wasm, pour 17 icônes : pré-rastériser ou police d'icônes. |
| **rmp-serde** (ajouté, §7 ter) | MessagePack pour le worker ; nécessaire car le record a des enums étiquetées (ni postcard ni bincode). |
| **rfd, dirs, serde_json, thiserror, env_logger** | Standards, rien à redire. |

Taille web : le wasm de l'UI est passé de 37 Mo à 10 Mo (SolidCore n'y est plus) ; le worker
fait 19 Mo. Reste à réduire, surtout le worker.

## Todo

### Fait

- [x] Bug « modifié » : comparé sur une révision de l'historique, plus sur la longueur du
      journal (annuler puis agir, réglages fusionnés passaient inaperçus).
- [x] `.idea/` dans `.gitignore`.
- [x] Noyau hors du thread de l'UI : `prepare` / `commit`, service `Inline` / `Threaded` / Web
      Worker, deux wasm, maillages asynchrones avec boîte englobante en attendant (§7 ter).
- [x] Doc : le tableau du §7 ne parle plus d'un callback egui_wgpu.

### À vérifier tout de suite

- [x] Tester la version web dans le navigateur (`./scripts/build-web.sh`, puis
      `python3 -m http.server 8080 -d dist/web`) : réseau de cylindres (`50x`),
      « calcul en cours… », Échap — validé le 2026-11-09.
- [x] Provoquer une panique du noyau dans le worker et vérifier la relance : menu
      Débogage › Faire planter le noyau, avec `?debug` dans l'adresse (web) ou
      `SOLIDSKETCH_DEBUG=1` (desktop) — validé sur le web le 2026-11-10.
- [x] Tester le desktop en vrai — validé le 2026-11-09.
- [ ] Commiter le noyau asynchrone et la suite (branche `async-kernel`).

### Chantier : pièces, assemblages, usinages (ARCHITECTURE §2)

Décidé le 2026-11-10 (risques 2 et 3). Par étapes, chacune testée avant la suivante.

- [x] Décisions écrites dans ARCHITECTURE §2, §4, §7 bis, §8.
- [x] **0. Vérifier SolidCore** (2026-11-10, `crates/kernel/tests/part_model.rs`) : le miroir
      dans T fonctionne ; soustractions successives retenues (réunir les outils d'abord n'est
      pas plus rapide) ; échecs d'usinage isolés. Trouvé au passage : **toute rotation oblique
      suivie d'un booléen échouait** (bug de cohérence SolidCore, contourné par
      `square_frames`).
- [ ] Rapporter le bug d'axes « pas d'équerre » à SolidCore (transform → relecture refusée).
- [x] Outil qui avale le brut : « résultat vide » au lieu de l'erreur de conversion « shell
      count » (2026-11-10, testé dans `part_model.rs`).
- [x] **1. Modèle** (`model`, 2026-11-10) : `Component` (pièce / assemblage), `Occurrence`,
      `Machining` ; opérations ; plus de groupes ni de solides libres ; purge des pièces sans
      exemplaire non nommées ; T rigide (miroir permis, échelle refusée).
- [x] **2. Fichier version 2** (2026-11-10) : fichiers de référence v1 et v2 relus dans les
      tests, conversion v1 → v2 à l'ouverture (vérifiée aussi sur un vrai fichier v1).
- [ ] **3. Noyau** : tâche « recette » (brut + usinages → B-rep final + affichage), cache
      identifié par la recette, échec par usinage.
- [x] **4. Scène et rendu** (2026-11-10) : unité = pièce, instanciation de tous ses
      exemplaires, miroir (sens des faces, testé).
- [ ] **5. Outils** — fait : déplacer / pivoter = changer T ; copier et réseau sans noyau ;
      soustraire → usinage ; union / intersection destructives ; **portée « tous les
      exemplaires / celui-ci »** des booléens (règles de Smart Reshape, §2.5), dans une **barre
      d'options de l'outil** (§6 bis, réglages mémorisés par outil, Tab), avec « ×N » et les
      autres exemplaires mis en évidence ; « Copier » visible pour Déplacer / Faire pivoter
      (2026-11-10). Reste : intégrer un usinage ; outils dans les axes de la pièce (option
      « Axes »).
- [ ] **6. Interface** — fait : arborescence pièces / assemblages, « ×N », nom d'exemplaire,
      nombre d'usinages, « Créer un assemblage », « Faire une pièce à part », exemplaires mis
      en évidence pendant un booléen. Reste : matière par exemplaire, propriétés, liste des
      usinages d'une pièce.
- [ ] Tester en vrai la barre d'options (desktop et web) : portée, Tab, « ×N », mise en
      évidence, mémorisation après redémarrage.
- [x] Desktop testé en vrai sur le nouveau modèle (2026-11-10).
- [x] Version web reconstruite et testée dans le navigateur (2026-11-10). Le build saute
      `wasm-pack` quand le wasm n'a pas changé (`scripts/build-web.sh`).

Ensuite, hors de ce chantier : types d'usinage paramétrés, usinages liés à une quincaillerie,
fiche de débit.

### Architecture

- [x] Découper `Editor` (risque 6, 2026-11-10) : `editor/` — `mod.rs` (état, outils),
      `document.rs` (opérations, noyau, annuler), `selection.rs`, `navigation.rs`, `view.rs`.
- [ ] Découper `infer.rs` (~1 270 lignes) avant pousser/tirer.
- [ ] Plafond mémoire de l'historique (et donc du cache de maillages).

### Fichier et noyau

- [ ] Fichiers de référence versionnés pour le format, relus dans les tests (risque 4) —
      dans l'étape 2 du chantier.
- [ ] Batterie de tests de robustesse SolidCore avant pousser/tirer.
- [ ] Mirror ou fork de SolidCore sous notre contrôle.
- [ ] Format binaire ou compressé, quand la taille des fichiers gênera.

### Web et build

- [ ] Réduire les wasm : mesurer la part de SolidCore, `opt-level = "s"` pour le worker,
      icônes sans `resvg`.
- [ ] CI : tests, clippy natif et wasm, build web (les tests `render` demandent un GPU).
- [ ] Trancher `rust-version = "1.88"` contre la toolchain figée en 1.97.1 (tester l'MSRV ou
      retirer la ligne).

### Plus tard

- [ ] Internationalisation : sortir les textes français de `model`.
- [ ] Paramétrique : nommage persistant des faces, si on le veut vraiment (risque 5).
