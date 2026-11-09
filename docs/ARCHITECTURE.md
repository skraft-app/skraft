# SolidSketch — architecture

Prototype d'outil de CAO 3D par solides, 100 % Rust, natif et WebAssembly. Moteur géométrique :
[SolidCore](https://github.com/tangency-labs/solidcore) (B-rep exact, port Rust de SMLib), celui
de [SolidCraft](https://github.com/tangency-labs/solidcraft). Modélisation proche de SketchUp :
**Solides (B-rep) → Groupes → Composants**, dans une arborescence.

Décisions prises le 2026-11-09 (questions/réponses).

## 1. Paradigme : modélisation directe, ouverte au paramétrique

- On modélise **directement** : pousser/tirer, déplacer, booléens appliqués immédiatement. Le
  B-rep *est* l'état ; il n'y a pas d'historique de reconstruction.
- Pour ne pas fermer la porte au paramétrique, **toute modification du document passe par une
  `Operation` typée et sérialisable** (`AddPrimitive`, `Transform`, `Boolean`, `PushPull`,
  `MakeGroup`, …), jamais par mutation directe. Aujourd'hui les opérations alimentent l'annulation
  et un journal ; demain, une timeline pourra les rejouer (le verrou étant alors le nommage
  persistant des faces).

## 2. Modèle de données

```
Document
├─ definitions : Id → Arc<Definition>
├─ root        : Id d'une Definition (le modèle lui-même)
└─ next_id

Definition { id, name, kind: Root | Group | Component, entities: Vec<Entity> }
Entity      = Solid    { id, name, brep: Arc<solidcore_record::Brep> }
            | Instance { id, name, definition: Id, transform: DAffine3 }
```

- **Un solide peut vivre dans n'importe quelle définition**, racine comprise. Il n'a pas de
  transformation propre : ses coordonnées sont celles de la définition qui le contient (le
  déplacer transforme son B-rep, comme une arête SketchUp).
- **Seules les instances portent une transformation.** Un *groupe* est une définition de genre
  `Group` à instance unique ; un *composant* est une définition de genre `Component` partageable
  par plusieurs instances. Convertir groupe ↔ composant ne change que le genre ; « rendre
  unique » duplique la définition.
- Modifier une définition de composant modifie **toutes** ses instances.
- Identifiants **stables `u64`** pour définitions, solides et instances. Une opération désigne
  une face par `(id du solide, index de face du record)` — suffisant en modélisation directe
  puisque l'opération s'applique tout de suite.
- **Chemin d'instance** : une entité dans la scène est désignée par le chemin des instances qui
  y mènent depuis la racine (`[inst₁, inst₂, …, entité]`) ; sa transformation monde est le
  produit des transformations du chemin.
- **Contexte actif** (comme « entrer dans le groupe » de SketchUp) : la définition en cours
  d'édition, et le chemin qui y mène.

## 3. Stockage des solides : record SolidCore

- Un solide est un **`solidcore_record::brep::Brep`** : données pures, `Clone` + `serde`,
  partagé par `Arc`. Le noyau (`solidcore::Kernel` + `solidcore::Brep`) n'est instancié que le
  temps d'une opération : record → noyau → opération → record.
- Avantages : instantanés d'annulation quasi gratuits, sauvegarde directe, aucun état noyau à
  maintenir. Coût : une conversion par opération. Si elle devient un goulot, on ajoutera un
  cache paresseux de corps noyau, invalidé à chaque modification.

## 4. Booléens entre contextes

- La **cible est modifiée dans son propre contexte** : l'outil est ramené dans le repère de la
  cible par la transformation relative `inv(monde(cible)) · monde(outil)`, puis l'opération
  remplace la cible par le résultat.
- L'outil est consommé (supprimé de son contexte) ou conservé, selon une option.
- Si la cible est dans un composant, toutes ses instances changent : c'est voulu.

## 5. Annuler / rétablir : instantanés partagés

- Le document est **immuable par morceaux** (`Arc` sur les définitions et les B-rep). Appliquer
  une opération produit un nouveau `Document` qui partage tout ce qui n'a pas changé.
- Annuler = revenir au document précédent ; rétablir = avancer. Aucune inverse à écrire.
- Le journal des opérations appliquées est conservé à côté (pour le futur paramétrique et le
  débogage).

## 6. Interaction : outils à la SketchUp

- Un **outil actif** (Sélection, Boîte, Cylindre, Déplacer, Rotation, Pousser/tirer, Orbite…)
  reçoit les événements du viewport (survol, clic, glisser, touches) et produit, à la fin, une
  `Operation`. Pendant l'interaction, il expose un **aperçu** (géométrie fantôme) sans toucher
  au document.
- **Navigation SketchUp** (décidé le 2026-11-09) : bouton du milieu = orbite autour du point
  visé, Maj+milieu = panoramique, molette = zoom vers le curseur, double-clic milieu =
  recentrer ; trackpad : deux doigts = orbite, Maj+deux doigts = pan, pincer = zoom. Z reste
  vertical. Projection perspective par défaut, bascule en parallèle, vues standard (dessus,
  face, droite, arrière, gauche, iso) et zoom étendu.
- **Sélection SketchUp** : un clic sélectionne l'entité de premier niveau du contexte actif,
  Maj+clic ajoute/retire, double-clic sur un groupe/composant y entre, double-clic dans le vide
  en sort, Échap annule l'outil puis vide la sélection. Hors du contexte actif, le modèle est
  estompé.
- **Booléens = outil en deux clics** (cible, puis outil), qui désigne des solides de n'importe
  quel contexte : c'est ainsi que s'exprime la règle du §4.
- **Saisie de cote au clavier** (VCB) : pendant un outil, taper une valeur (`250`, `30;40;50`)
  fixe la dimension.
- **Copie et réseaux à la SketchUp** (Déplacer, Faire pivoter ; décidé le 2026-11-09) : un
  appui bref sur le modificateur de copie (Option sur macOS, Ctrl ailleurs), seul, bascule la
  copie, avant ou pendant le geste ; le curseur porte un « + » et la copie retombe une fois
  faite. Juste après une copie, taper `5x` (ou `*5`) en fait un réseau de 5 au même pas, `/5`
  répartit 5 copies sur le même chemin ; on peut retaper pour corriger. Le réseau est une
  opération (`Operation::Array` : `count` copies, la k-ième transformée k fois par `step`),
  qui remplace la copie dans l'historique (`Command::Replace`) : une seule étape d'annulation.
  Translation comme rotation (réseau circulaire).
- Inférences : voir §6 ter.

## 6 bis. Interface de l'application

Décidé le 2026-11-09.

- **Agencement** : menu en haut, barre d'état en bas avec la saisie de cote (VCB) à droite ;
  entre les deux, la **vue 3D occupe toute la largeur** et les panneaux **flottent dessus**
  (cartes ancrées) : **barre d'outils verticale à gauche** ; à droite, **Infos sur
  l'entité** au-dessus de l'**Arborescence**, repliables. Ouvrir ou redimensionner un panneau ne
  redimensionne donc jamais la vue ; le zoom étendu cadre sur la zone laissée libre.
- **Icônes SVG originales** (dessinées pour le projet), blanches et teintées par le thème,
  chargées par `egui_extras` (`svg`) : ni étape de build, ni asset binaire.
- **Raccourcis SketchUp** : Espace = Sélection, M = Déplacer, Q = Faire pivoter, G = Créer un composant,
  Suppr = Effacer, Cmd/Ctrl+Z / Maj+Cmd+Z = Annuler / Rétablir, Maj+Z = Zoom étendu ; les
  nôtres : B = Boîte, C = Cylindre, Cmd/Ctrl+1…6 = vues standard, Cmd/Ctrl+, = Préférences. Une fois un chiffre tapé,
  la frappe va à la VCB (unités comprises : `cm`, `m`…), validée par Entrée.
- Le viewport est l'image de la cible hors écran de `render`, enregistrée comme texture egui ;
  le rendu est soumis pendant `update`, avant la peinture d'egui.
- Molette (unités « ligne ») = zoom ; défilement trackpad (unités « point ») = orbite,
  Maj = pan ; pincer = zoom. Modificateur de copie : Option sur macOS, Ctrl ailleurs (appui bref = bascule, §6).
- **Réglages à deux niveaux** (décidé le 2026-11-09, sur le modèle des styles de SketchUp) :
  - **réglages du document** (`model::Settings`) : affichage (grille — masquée par défaut —,
    axes, couleur de fond) et unités (unité des longueurs, décimales). Enregistrés dans le
    fichier ; les changer est une opération (`Operation::SetSettings`), donc annulable, et
    des changements successifs (glisser une couleur) fusionnent en un seul pas. Les longueurs
    restent stockées en mm : l'unité ne sert qu'à la saisie (un nombre nu est dans l'unité du
    document) et à l'affichage ;
  - **préférences de l'utilisateur** (`apps/solidsketch/src/prefs.rs`) : navigation (vitesse
    et sens de l'orbite, souris et trackpad séparément ; vitesse, sens et cible du zoom) et
    les réglages des nouveaux modèles. Fichier JSON versionné à nous,
    `preferences.json` dans le dossier de configuration de l'OS
    (`~/Library/Application Support/SolidSketch/` sur macOS), `localStorage` dans le
    navigateur ; réécrit dès qu'une préférence change.
  - Partout `#[serde(default)]` : un fichier plus ancien ou partiel se charge, un nouveau
    réglage ne demande pas de migration.
  - Fenêtre **Préférences** (Cmd/Ctrl+,) : onglet *Modèle* (réglages du document, avec
    « Utiliser pour les nouveaux modèles » / « Reprendre les réglages par défaut ») et onglet
    *Navigation*. *Vue › Grille / Axes* change aussi le document.

## 6 ter. Inférences

Décidé le 2026-11-09.

- **Un moteur central** (`tools/src/infer.rs`, sans UI ni GPU) remplace les choix de point
  propres à chaque outil (`base_point`, `target`, `center_at`…). Un outil demande
  `cx.infer(px, &Query { from, lock, exclude })` et reçoit une `Inference { point, kind, guides,
  path }` : point monde, genre d'inférence, lignes de guidage et solide d'appui éventuel.
- **Géométrie exacte** : les candidats viennent du **record B-rep** (sommets `f64`, courbes
  typées : milieu d'une droite, centre d'un cercle exacts), pas du maillage. Un `SnapIndex` par
  solide (sommets, arêtes et leur courbe, boîte englobante) est mis en cache par `Arc<Solid>`
  comme les maillages ; il est transformé par chaque placement. Repli sur la polyligne du
  mailleur pour les splines et courbes d'intersection (`Mesh::edges` doit garder l'index
  d'arête du record).
- **Priorités** (à priorité égale, le plus proche du curseur à l'écran, tolérance ≈ 10 px) :
  extrémité > milieu / centre / origine > intersection (contrainte linéaire × arête ou face) >
  linéaire (axes rouge / vert / bleu depuis `from`, parallèle / perpendiculaire à une arête) >
  sur arête > sur face (projeté exactement si la face est plane) > plan du sol ou plan par
  `from`.
- **Occlusion** : un sommet ou une arête derrière la surface touchée par le rayon n'est pas
  candidat (à une tolérance près) ; les arêtes de la face survolée restent accessibles.
  L'inférence porte sur toute la scène visible, y compris hors du contexte actif.
- **Verrous**, tenus par l'éditeur pour tous les outils : flèches → / ← / ↑ = axe rouge / vert /
  bleu, ↓ = libre ; Maj maintenue verrouille l'inférence linéaire en cours. Un point survolé
  pendant un verrou est projeté sur la ligne (alignement). Les verrous tombent à chaque
  opération appliquée, à Échap, au changement d'outil et à l'annulation. Selon l'outil, le
  verrou d'axe donne la direction (Déplacer), l'axe de rotation (Rotation) ou la normale du plan
  de dessin (Boîte, Cylindre : ↑ horizontal, → perpendiculaire au rouge, ← au vert).
- **Lignes** : passant par le point de départ de l'outil, axes du contexte et, pour l'arête
  droite survolée en dernier, parallèle et perpendiculaire (dans le plan de l'outil). Une ligne
  presque alignée sur la direction de vue est ignorée (elle n'est qu'un point à l'écran). Si
  elle perce la face sous le curseur près de lui, c'est ce point qui est pris.
- **Retour visuel** : lignes de guidage (couleur de l'axe) par l'overlay existant ; marqueurs
  colorés par genre et info-bulle en français (« Extrémité », « Milieu », « Sur l'axe rouge »…)
  peints par egui par-dessus le viewport (`camera.project`) : `render` ne change pas.
- **Plan de dessin** : Boîte et Cylindre se dessinent dans le plan de la face survolée au
  premier clic (hauteur selon sa normale), comme SketchUp ; sinon sur le sol du contexte.
  `Operation::AddPrimitive` reçoit pour cela un repère rigide (`placement: DAffine3`, identité
  par défaut en serde pour relire les fichiers existants) ; la primitive est construite alignée
  sur les axes puis transformée.
- **Performances** : force brute avec élimination par boîtes englobantes contre le cône de
  sélection ; une BVH seulement si la mesure l'exige.
- **Étapes** : (1) points — extrémité, milieu, centre, sur arête, sur face, sol — avec
  marqueurs, info-bulle et branchement dans Boîte, Cylindre, Déplacer, Rotation ;
  (2) linéaire — axes depuis `from`, parallèle / perpendiculaire, verrou Maj, flèches
  factorisées. **Livrées ensemble.** Plus tard : (3) suivi de points (références temporaires,
  intersection de deux contraintes), tangente aux arcs.
- Tests sans affichage, caméra fixe : chaque genre, priorités, occlusion, inférence dans un
  composant transformé, scène à 100 km de l'origine.
- **État (2026-11-09)** : étapes 1 et 2 faites — `kernel::Snaps` (construit avec le maillage
  par `Solid::display()`, mis en cache par solide dans la scène), `tools::infer`, lignes et
  verrous, marqueurs et info-bulle dans l'app, branchement dans Boîte, Cylindre, Déplacer,
  Rotation ; Boîte et Cylindre orientés sur la face (`AddPrimitive::placement`). Reste
  l'étape 3 : suivi de points, intersection d'une ligne avec une arête, tangente aux arcs.
  Les **coutures** (arête entre une face et elle-même, ou entre deux faces d'une même
  surface) sont reconnues sur le record : le mailleur ne remplit pas `seams`, si bien que la
  couture d'un cylindre était dessinée jusque-là.

### Étape 3 — inférences relatives (décidé le 2026-11-09)

- **Suivi de points** : un point (extrémité, milieu, centre, origine) survolé **environ 0,5 s**
  devient une **référence temporaire**, sans clic ni touche, comme dans SketchUp. L'app transmet
  l'heure à l'éditeur avec chaque survol. On en garde **deux**, les plus récentes ; chacune
  reste marquée à l'écran. Elles tombent quand une opération est appliquée, à Échap et au
  changement d'outil.
- **Lignes depuis les références** : en plus de celles qui partent du point de départ, le
  moteur infère les axes rouge / vert / bleu passant par chaque référence (« Depuis le point,
  sur l'axe rouge », guide pointillé partant de la référence).
- **Intersections** (rang « point construit », juste après milieux et centres) : ligne × ligne
  (deux axes de sources différentes) et ligne × arête. Deux droites 3D étant rarement
  sécantes, l'intersection est acceptée quand leur écart minimal est sous une fraction de pixel
  ramenée à la profondeur ; le point retenu est pris sur la ligne la plus forte (un axe), pour
  rester exactement dessus. Ligne × face : déjà fait.
- **Tangente aux arcs** : un point sur un arc donne sa tangente exacte comme arête de
  référence ; d'où « Tangente à l'arc » (parallèle à la tangente par le point de départ) et
  « Perpendiculaire à l'arc » (vers le centre).
- **Livraison progressive**, chaque partie testée avant la suivante : (a) tangente aux arcs,
  (b) ligne × arête, (c) suivi de points et ligne × ligne.
- **État** : (a) fait — un point sur un arc (ou son milieu) devient la référence
  `EdgeRef::Arc { tangent, radial }` au point exact ; une arête droite, `EdgeRef::Straight`.
  Une tangente ou un rayon confondu avec un axe est annoncé comme l'axe.
  (b) fait — les genres linéaires sont regroupés en `LineKind` (axe, parallèle,
  perpendiculaire, tangente, rayon) : `InferKind::OnLine(l)` sur une ligne,
  `InferKind::OnEdgeAndLine(l)` où elle croise une arête (segment, arc — dans son plan ou en le
  traversant —, polyligne), à moins d'un demi-pixel ramené à la profondeur ; le point est pris
  sur la ligne. Sous un verrou, l'arête croisée près du curseur donne ce croisement avant toute
  projection. Marqueur : le carré de l'arête, dans la couleur de la ligne ; info-bulle
  « Arête × axe rouge »…
  (c) fait — l'éditeur reçoit l'heure de l'app (`Editor::tick`, appelé à chaque image ; il
  demande un rafraîchissement tant qu'un point attend) ; un point de la géométrie (extrémité,
  milieu, centre, origine) survolé `TRACK_AFTER` = 0,5 s est suivi, `MAX_TRACKED` = 2. Ses axes
  sont des lignes `LineKind::TrackedAxis(k)` (« Depuis le point, sur l'axe rouge ») ; deux
  lignes d'origines différentes qui se croisent à moins d'un demi-pixel donnent
  `InferKind::OnTwoLines(a, b)`, le point pris sur la première (celle du départ s'il y en a
  une). Croisement en 3D strict : deux lignes à des hauteurs différentes ne se croisent pas,
  même superposées à l'écran. Le verrou (`Lock`) retient l'origine de sa ligne : Maj tient
  aussi une ligne issue d'un point suivi. Les points suivis sont marqués d'un anneau ; les
  guides qui en partent sont en pointillés. Ils tombent avec les verrous.

## 7. Découpage en crates

```
                   ┌──────────────┐
                   │    kernel    │  SolidCore derrière une API sûre
                   └──────┬───────┘
              ┌───────────┴───────────┐
       ┌──────┴──────┐          ┌─────┴──────┐
       │    model    │          │   render   │  wgpu, sans egui
       └──────┬──────┘          └─────┬──────┘
       ┌──────┴──────┐                │
       │    tools    │                │
       └──────┬──────┘                │
              └──────────┬────────────┘
                  ┌──────┴──────┐
                  │ app (egui)  │  câblage, panneaux, viewport
                  └─────────────┘
```

| Crate | Rôle | Dépend de |
|---|---|---|
| `crates/kernel` — `solidsketch-kernel` | `Solid` (record), primitives, booléens, transformation, décalage de faces, maillage d'affichage (`Mesh` : positions, normales, indices, face B-rep par triangle, arêtes). Exécute chaque appel sur un `Kernel` neuf et attrape ses paniques. | `solidcore`, `solidcore-record`, `solidcore-convert`, `solidcore-tess`, `glam` |
| `crates/model` — `solidsketch-model` | `Document`, `Definition`, `Entity`, ids, chemins d'instance, `Operation` + `apply`, historique d'annulation, fichier JSON versionné. Aucune dépendance UI ; testable sans affichage. | `kernel`, `serde` |
| `crates/render` — `solidsketch-render` | Pipelines wgpu (faces ombrées, arêtes, grille, axes, surbrillance, aperçus), cache GPU des maillages par solide, dessin d'une liste `DrawItem { mesh, transform, style }` avec une matrice vue-projection. Ne connaît ni egui ni le document. Utilisable hors écran. | `kernel` (pour `Mesh`), `wgpu`, `glam` |
| `crates/tools` — `solidsketch-tools` | Caméra (orbite/pan/zoom, rayon depuis l'écran), scène d'affichage (aplatissement du document en solides placés + cache de maillages), sélection par rayon (CPU, triangle → face B-rep), machines à états des outils, saisie de cote, inférences. Testable sans UI ni GPU. | `model`, `kernel`, `glam` |
| `apps/solidsketch` | eframe : barre d'outils, arborescence, propriétés, barre d'état + VCB, viewport (callback de peinture `egui_wgpu` vers `render`), traduction des événements egui en événements d'outils, ouverture/enregistrement. Natif et wasm. | tout ce qui précède, `eframe`, `egui-wgpu` |

## 7 bis. Affichage : la définition comme unité de rendu

Décidé le 2026-11-09. Un composant est un maillage instancié : l'affichage suit le modèle.

```
Niveau 1 (CPU, tools)  Arc<Solid> → Arc<Mesh>      tessellation en cache par solide : un
                                                   solide inchangé n'est jamais re-tessellé
Niveau 2 (GPU, render) Definition → GpuMesh        concaténation des maillages de SES solides
                       + plages [solide → indices] directs ; re-fusionnée (copie) si elle change
Dessin                 1 draw instancié par définition, avec les N matrices monde de toutes
                       ses occurrences dans la scène (imbrication comprise)
```

- Le buffer d'une définition ne contient que ses **solides directs** ; ses sous-groupes ont le
  leur, dessiné avec les transformations composées.
- **Surbrillance** : un solide sélectionné l'est sur un chemin (une seule occurrence) ; il est
  redessiné par-dessus avec sa plage d'indices et la matrice de cette occurrence.
- **`render` ne connaît pas le document** : il reçoit des maillages par clé (l'id de la
  définition) avec une révision, et une liste de lots `(clé, matrices)` + surbrillances +
  lignes d'aperçu. C'est `tools` (la scène d'affichage) qui aplatit le document et décide des
  révisions.
- `render` dessine dans **sa propre cible hors écran** (couleur MSAA ×4 + profondeur, résolue
  dans une texture) : l'app l'affiche comme une image egui. Le rendu est ainsi indépendant
  d'egui et testable sans fenêtre.
- **Précision** (le GPU n'a que du `f32`, ni WebGPU, ni WebGL2, ni Metal n'offrent de `f64`) :
  - **rendu relatif à la caméra** : vue, placements et lignes sont en `f64` ; chaque
    `vue × monde × recentrage` est calculé en `f64` sur le CPU puis seulement converti en
    `f32` (le buffer d'instances est réécrit à chaque image de toute façon) ; le shader
    n'applique que la projection ;
  - **maillages recentrés** : le `Mesh` du noyau est en `f32` relatif à son centre (`origin`
    en `f64`), et le maillage GPU d'une définition est relatif au centre de ses solides ;
  - **profondeur inversée** (proche → 1, loin → 0) dans un `Depth32Float` : précision quasi
    uniforme ; la projection fournie doit être inversée
    (`Mat4::perspective_infinite_reverse_rh`).
  - Vérifié par un test : la même scène à 100 km de l'origine donne la même image.
- **Traits épais, en points** : les lignes matérielles font un pixel (un demi-point sur un
  écran haute densité). Arêtes, axes, grille et aperçus d'outils sont donc des quads (deux
  triangles par segment) élargis à l'écran par le vertex shader, après un découpage du
  segment devant l'œil. Les largeurs sont en points (`EDGE_WIDTH` 1,5 ; `LINE_WIDTH` 1,5 ;
  `GRID_WIDTH` 1) et multipliées par la densité de pixels de l'écran
  (`Frame::pixels_per_point`) : même épaisseur apparente partout. Le quad a la profondeur de
  sa ligne centrale, alors qu'à un pixel de là une face voisine peut être plus proche (coins
  rentrants, faces rasantes) : les traits sont donc **tirés vers l'œil** de 3 demi-largeurs
  mesurées à leur profondeur (homothétie vers l'œil en perspective, translation sur l'axe de
  vue en parallèle : le pixel ne bouge pas). Test : les arêtes d'une poche ne sont pas
  enfouies. WebGPU n'a pas de largeur de ligne (Metal n'en a pas du tout).

## 8. Fichier

JSON versionné (`{"format": "solidsketch", "version": 1, …}`) : sérialisation serde du
document, B-rep compris (format record de SolidCore), et ses réglages (`settings`, absent des
premiers fichiers : valeurs par défaut). Dans le navigateur : téléchargement et
chargement de fichier.

## 9. Chaîne d'outils et dépendances

- Rust **1.97.1** figé par `rust-toolchain.toml` (les crates SolidCore demandent ≥ 1.88), avec
  la cible `wasm32-unknown-unknown`.
- `solidcore` est sur crates.io, mais `solidcore-record`, `-convert` et `-tess` ne le sont pas :
  les quatre viennent du miroir git `tangency-labs/solidcore`, **figés sur un même commit**.
- wasm : pile de 8 Mio (`.cargo/config.toml`) — SolidCore récurse légitimement et wasm n'a pas
  de page de garde. Les paniques du noyau ne sont pas rattrapables en wasm (`panic = abort`) :
  à surveiller.

## 10. Périmètre du prototype v0, par étapes

1. **Socle** : `kernel` (fait, à finaliser), `model` (document, opérations, undo, JSON),
   `render` + app : viewport (orbite/pan/zoom, grille, axes), arborescence, sélection,
   primitives, déplacer/rotation, annuler/rétablir ; build natif + wasm.
2. **Groupes & composants** : créer depuis la sélection, éclater, entrer/sortir d'édition,
   rendre unique, instances multiples.
3. **Booléens** : union, différence, intersection, y compris entre contextes (§4).
4. **Pousser/tirer** : sélection d'une face B-rep, décalage (`offset_faces` du noyau, ou
   extrusion + booléen). Le plus risqué côté noyau : en dernier.
