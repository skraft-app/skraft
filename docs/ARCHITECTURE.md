# SolidSketch — architecture

Prototype d'outil de CAO 3D par solides, 100 % Rust, natif et WebAssembly. Moteur géométrique :
[SolidCore](https://github.com/tangency-labs/solidcore) (B-rep exact, port Rust de SMLib), celui
de [SolidCraft](https://github.com/tangency-labs/solidcraft). Modélisation proche de SketchUp,
pensée pour la menuiserie : **pièces** (B-rep usinés) et **assemblages**, tous deux des
**composants**, dans une arborescence. Objectif à terme : une fiche de débit et un calepinage
comme [OpenCutList](https://github.com/lairdubois/lairdubois-opencutlist-sketchup-extension).

Décisions prises le 2026-11-09, et le 2026-11-10 pour le modèle pièces / assemblages (§2),
**implémenté en partie** : ce qui reste est listé au §2.8 et dans `docs/REVUE.md`.

## 1. Paradigme : modélisation directe, ouverte au paramétrique

- On modélise **directement** : pousser/tirer, déplacer, booléens appliqués immédiatement. Le
  B-rep *est* l'état ; il n'y a pas d'historique de reconstruction.
- Pour ne pas fermer la porte au paramétrique, **toute modification du document passe par une
  `Operation` typée et sérialisable** (`AddPrimitive`, `Transform`, `Boolean`, `PushPull`,
  `MakeAssembly`, …), jamais par mutation directe. Aujourd'hui les opérations alimentent l'annulation
  et un journal ; demain, une timeline pourra les rejouer (le verrou étant alors le nommage
  persistant des faces).

## 2. Modèle de données : pièces, assemblages, usinages

Décidé le 2026-11-10, en place depuis le même jour (sauf le §2.8) ; l'ancien modèle est résumé
au §2.7.

```
Document
├─ components : Id → Arc<Component>
├─ root       : Id de l'assemblage racine (le modèle, jamais compté lui-même)
└─ next_id

Component { id, name, props, kind }
  props   : type (bois massif, panneau, quincaillerie…), exclu de la fiche de débit, …
  kind    = Part     { blank: Arc<Brep>, machinings: Vec<Machining> }      — une pièce
          | Assembly { occurrences: Vec<Occurrence> }                      — un assemblage
Occurrence { id, component: Id, transform: T, material, name }            — un exemplaire
Machining  { id, tool: Arc<Brep>, placement: T, source: Local }            — un usinage
            (plus tard : source = Linked { … }, §2.4)
```

### 2.1 Composants : pièces et assemblages

- **Un seul mécanisme de partage** : un composant est une définition partagée et nommée, placée
  par des exemplaires. Une **pièce** contient un B-rep (son brut et ses usinages) ; un
  **assemblage** contient des exemplaires d'autres composants. « Composant » est le terme
  commun, que l'interface emploie peu : elle parle de pièces et d'assemblages.
- **Plus de groupes**, plus de géométrie libre : tout ce qui est dans le modèle est un
  exemplaire. Dessiner une boîte crée une pièce et son premier exemplaire ; rassembler des
  exemplaires crée un assemblage (nommé « Assemblage N » tant que l'utilisateur ne le renomme
  pas).
- Une pièce s'édite **directement** (pas d'« entrer dedans ») ; un assemblage s'ouvre comme un
  composant SketchUp (contexte actif, §2.6).
- Les **propriétés** (type, exclusion de la fiche…) se posent sur n'importe quel composant : une
  charnière modélisée en plusieurs pièces est un assemblage de type quincaillerie, compté comme
  un article sans détailler son contenu.
- **Nom** : celui du composant est le nom de la pièce ; le **nom d'exemplaire** nomme un
  exemplaire de cette pièce (« Gauche », « Droite »), comme dans OpenCutList.
- **Purge** : une pièce sans exemplaire disparaît, sauf si l'utilisateur l'a nommée (elle reste
  alors dans la bibliothèque du modèle).

### 2.2 Exemplaires et transformation

- Un exemplaire porte **T = déplacement + rotation + miroir éventuel** (déterminant ±1), sa
  **matière** et son nom d'exemplaire. Déplacer ou faire pivoter ne change que T : instantané,
  sans noyau, sans remaillage, sans dérive de la géométrie.
- **T n'est jamais intégré au B-rep.** Booléens et (plus tard) pousser/tirer travaillent dans le
  repère de la pièce, l'autre opérande ramené par la transformation relative ; l'export applique
  T à la volée.
- **Pas d'échelle dans T.** Mettre à l'échelle (uniformément) change les cotes, donc la pièce :
  c'est une modification de géométrie (§2.5). L'**échelle non uniforme est interdite** partout :
  elle casse cercles, inférences et cotes.
- Le **miroir reste dans T** : une pièce et sa symétrique restent la même pièce (la fiche les
  distingue, sauf si la pièce est déclarée symétrique). Vérifié le 2026-11-10
  (`crates/kernel/tests/part_model.rs`) : SolidCore applique une symétrie, seule ou avec une
  rotation, à toutes les géométries (plans, cylindres, sphères, cônes, tores, courbes
  d'intersection), retourne correctement les faces, et le solide symétrique sert ensuite de
  cible comme d'outil ; couper puis symétriser = symétriser puis couper.

### 2.3 Axes d'une pièce

- Le repère du B-rep **est** celui de la pièce : la longueur suit l'axe rouge, et le **fil est
  toujours la longueur** (pas de propriété à part). Les cotes de débit sont lues sur la boîte
  englobante du brut dans ce repère.
- Les outils travaillant sur une pièce utilisent ses axes ; ailleurs, ceux du contexte.
- Changer les axes d'une pièce = réexprimer son B-rep dans un autre repère et compenser le T de
  chacun de ses exemplaires.

### 2.4 Usinages

- Une pièce est **une recette** : **brut** − (outil₁ ∪ outil₂ ∪ …). Le **B-rep final** est
  calculé par le noyau (asynchrone, §7 ter) et mis en cache, identifié par la recette ; il n'est
  jamais la source.
- **Toujours de l'enlèvement de matière** : l'ordre des usinages ne compte pas (un ensemble pour
  l'utilisateur), le calcul peut être incrémental, et un usinage qui échoue est isolé et signalé
  sans bloquer les autres.
- **Calcul : soustractions successives, incrémentales.** Mesuré (planche − n perçages, build de
  dev) : réunir d'abord les outils puis couper une fois n'est pas plus rapide (50 perçages :
  ~0,4–0,8 s dans les deux cas). Ajouter un usinage = une soustraction sur le final en cache ;
  en retirer un = repartir du brut (ou d'un final intermédiaire gardé en cache).
- **Échecs mesurés** : un outil hors du brut, affleurant une face, touchant une arête ou tangent
  ne fait rien d'anormal ; un outil qui avale tout le brut donne une erreur de conversion (à
  rapporter comme « résultat vide »).
- Bas niveau : l'outil est **un B-rep quelconque**, placé dans le repère de la pièce, ancré à son
  origine. Les **types d'usinage** (perçage, rainure, mortaise…) viendront au-dessus et
  généreront cet outil depuis leurs paramètres.
- **Liens, plus tard** : chaque usinage a un id stable et une `source`. Une quincaillerie fixée
  pourra porter ses outils et produire des usinages *liés* dans la pièce, placés par
  `T_pièce⁻¹ · T_quincaillerie · T_outil`, recalculés quand elle bouge. Le format prévoit le
  champ dès maintenant (toujours `Local` pour l'instant).

### 2.5 Copies et propagation

- **Copier un exemplaire crée un exemplaire de la même pièce** : « Pied ×4 », compté sans rien
  deviner, sans penser à « faire un composant » avant de copier.
- Une **modification de la pièce** (géométrie, usinages, propriétés) touche **tous ses
  exemplaires** ou **seulement celui-ci** : la **portée**, une option de l'outil (barre
  d'options, §6 bis), « tous les exemplaires » par défaut, mémorisée par outil. Décidé le
  2026-11-10, sur le modèle des opérations de solides de Smart Reshape (OpenCutList,
  `common_solid_boolean_apply_worker.rb`) :
  - **tous les exemplaires** (`make_unique: false` d'OCL) : la pièce est modifiée sur place, et
    tous ses exemplaires — y compris ceux qu'on voit à travers des copies d'un assemblage —
    montrent le résultat. L'opération est calculée dans le repère de l'exemplaire cliqué ;
  - **seulement celui-ci** (`make_unique`) : seul l'objet cliqué change. Sur son chemin, du haut
    vers le bas, chaque assemblage montré ailleurs devient un assemblage à part, puis la pièce
    devient une pièce à part ; les autres exemplaires ne sont jamais touchés ;
  - quand une opération aura **plusieurs cibles** (plus tard) : des exemplaires d'une même pièce
    que l'opération laisse identiques dans leur propre repère gardent une seule pièce ; s'ils
    divergent, ils deviennent des pièces à part même en « tous les exemplaires », la pièce ne
    pouvant porter deux résultats (règle d'OCL) ;
  - les **verrous** (plus tard) : un exemplaire verrouillé n'est jamais modifié par
    propagation, comme dans OCL.
- **La propagation doit être visible** : nombre d'exemplaires concernés dans la barre
  d'options (« Tous les exemplaires ×4 »), exemplaires concernés mis en évidence pendant une
  modification. « Rendre unique » devient **« Faire une pièce à part »**,
  avec une version en profondeur pour un assemblage.
- La **fiche de débit** peut réunir des pièces de mêmes cotes et matière (option d'affichage) ;
  le modèle, le calepinage et les exports les gardent toujours distinctes (mêmes cotes ne veut
  pas dire même forme).

### 2.6 Identifiants, chemins, contexte

- Identifiants **stables `u64`**, tirés d'un même compteur, pour composants, exemplaires et
  usinages. Ceux des composants et des usinages sont uniques dans le document ; ceux des
  **exemplaires** le sont **dans leur assemblage** : une copie d'assemblage (« faire une pièce à
  part », portée « seulement celui-ci ») garde les ids de ses exemplaires, si bien que les
  chemins, le contexte et la sélection restent valides. Un chemin est toujours résolu depuis la
  racine. Une opération désigne
  une face par `(id de la pièce, index de face du B-rep final)` — suffisant en modélisation
  directe puisque l'opération s'applique tout de suite.
- **Chemin d'exemplaire** : une entité de la scène est désignée par le chemin des exemplaires
  depuis la racine ; sa transformation monde est le produit des T du chemin.
- **Contexte actif** : l'assemblage en cours d'édition, et le chemin qui y mène.

### 2.7 Ancien modèle (fichiers version 1)

```
Definition { id, name, kind: Root | Group | Component, entities: Vec<Entity> }
Entity      = Solid    { id, name, brep: Arc<Brep> }             — coordonnées de sa définition
            | Instance { id, name, definition: Id, transform }
```

Solides libres dans n'importe quelle définition, sans transformation propre (les déplacer
réécrivait leur B-rep) ; groupes = définitions à instance unique. À l'ouverture d'un fichier v1
(`file.rs`, module `v1`) : chaque définition devient un assemblage (nommé pour un composant, pas
pour un groupe), chaque instance un exemplaire, et chaque solide libre une pièce — ses axes
ceux de sa définition, donc rien ne bouge et le noyau n'est pas appelé — avec son exemplaire,
qui garde l'id du solide.

### 2.8 Pas encore en place

- **Recette recalculée** : aujourd'hui, une soustraction coupe le final en cache et l'usinage
  est ajouté à la liste ; il n'y a encore ni retrait, ni modification, ni « intégrer » d'un
  usinage, donc pas de recalcul depuis le brut.
- **« Tous les exemplaires / seulement celui-ci »** et la pièce à part automatique : une
  modification de géométrie touche aujourd'hui tous les exemplaires de la pièce.
- **Matière par exemplaire, propriétés** (type, exclusion de la fiche) : absentes du format pour
  l'instant ; elles y entreront avec une valeur par défaut (les fichiers v2 resteront lisibles).
- **Outils dans les axes de la pièce** (§2.3), mise en évidence des exemplaires pendant une
  modification, liste des usinages dans l'interface.
- « Faire une pièce à part » sur un assemblage n'en copie que le premier niveau (pas en
  profondeur).

## 3. Stockage des solides : record SolidCore

- Un solide est un **`solidcore_record::brep::Brep`** : données pures, `Clone` + `serde`,
  partagé par `Arc`. Le noyau (`solidcore::Kernel` + `solidcore::Brep`) n'est instancié que le
  temps d'une opération : record → noyau → opération → record.
- Après chaque opération, le record est **remis d'équerre** (`square_frames`, `engine.rs`) :
  une rotation autour d'un axe oblique laisse des axes de plan à ~1e-15 de l'angle droit, que
  SolidCore refuse ensuite de relire (tolérance ~3e-16) — l'opération suivante échouait.
- Avantages : instantanés d'annulation quasi gratuits, sauvegarde directe, aucun état noyau à
  maintenir. Coût : une conversion par opération. Si elle devient un goulot, on ajoutera un
  cache paresseux de corps noyau, invalidé à chaque modification.

## 4. Booléens entre contextes

- La **cible est modifiée dans son propre repère** : l'outil est ramené dans le repère de la
  pièce cible par la transformation relative `inv(monde(cible)) · monde(outil)`.
- **Soustraire** (modèle du §2) : l'exemplaire outil devient un **usinage** de la pièce cible,
  sans détruire sa géométrie ; il quitte le modèle, les autres exemplaires de sa pièce restent.
  **Intégrer** applique un usinage au brut et le retire de la liste.
- **Union et intersection** sont destructives : un nouveau brut, fait des formes finales, où les
  usinages des deux pièces sont intégrés.
- La modification touche tous les exemplaires de la pièce cible ou seulement celui-ci (§2.5).
- Pas d'option « garder l'outil » : l'outil quitte toujours le modèle (il vit dans l'usinage).

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
  en sort, Échap annule l'outil puis vide la sélection. Glisser trace un rectangle de
  sélection : de gauche à droite (trait plein, « fenêtre »), il prend les entités qu'il contient
  entièrement ; de droite à gauche (pointillé, « capture »), celles qu'il touche ; Maj
  ajoute/retire. Le test se fait à l'écran, sur les sommets et triangles projetés des maillages
  d'affichage. Hors du contexte actif, le modèle est
  estompé.
- **Booléens = outil en deux clics** (cible, puis outil), qui désigne des solides de n'importe
  quel contexte : c'est ainsi que s'exprime la règle du §4.
- **Saisie de cote au clavier** (VCB ; décidé le 2026-11-10, repris d'OpenCutList,
  `UserTextHelper` + `LengthExpressionUtils`) : pendant un outil, taper une valeur fixe la
  dimension. On peut taper :
  - des expressions : `600-2*19`, `2m*(2+3)`, `min(@; 300)`, `floor(@ - 5; 12; 15; 20)` ;
  - `@`, la valeur courante du geste : `@/2`, `@+19` ;
  - des listes séparées par `;` (case vide = valeur courante : `;400`) ; `=` répète une
    valeur (`600=` = `600;600`) ;
  - des points : `[x;y;z]` dans les axes du contexte, `<dx;dy;dz>` depuis le point de
    départ (Déplacer).

  Un nombre nu est dans l'unité du document : le calcul s'y fait, donc `@*3` et `@-2` se
  lisent tous deux. Les unités sont contrôlées strictement (`2m*3m` et `@/@` sont refusés).
  Une longueur tapée garde le sens du geste. `,` et `.` sont tous deux des virgules
  décimales, donc `;` est le seul séparateur, et il ne coupe qu'hors parenthèses.
  L'évaluateur vit dans `model` (`expr`, `measure`), pour servir aussi aux champs (Position
  des propriétés) et plus tard aux cotes des pièces. Chaque outil déclare ce qu'il prend
  (`Tool::vcb` : longueurs, angle, nombre de copies, point accepté ou non) avec ses valeurs
  courantes ; l'éditeur lit la saisie et lui passe une `ToolInput::Value`, ou affiche
  l'erreur.
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
  nôtres : B = Boîte, C = Cylindre, Cmd/Ctrl+1…6 = vues standard, Cmd/Ctrl+, = Préférences. Une fois un chiffre tapé
  (ou `@ ( [ < + - * / ;`), la frappe va à la VCB, validée par Entrée.
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
- **Barre d'options de l'outil** (décidé le 2026-11-10, sur le modèle des outils d'OCL) : une
  bande fixe entre le menu et la vue (elle ne masque rien), avec le nom de l'outil et ses
  options — choix exclusifs et bascules. Tout choix y est visible et cliquable ; un raccourci
  n'en est qu'un complément (Tab fait tourner la portée). La barre d'état garde le guidage de
  l'étape en cours.
  - **Les outils décrivent leurs options, l'app les dessine** : `Tool::options` rend une liste
    de `ToolOption` (clé, libellé — qui peut dépendre de ce qui est survolé, « ×4 » —,
    info-bulle, valeur, choix), `Tool::set_option` reçoit un changement. `tools` reste sans
    interface : un test change une option comme un clic.
  - **Deux sortes d'options** : les **réglages** (la portée…), mémorisés par outil dans les
    préférences de l'utilisateur (`tool_options`), lus par l'outil dans son `Cx` ; et l'**état
    du geste** (copier, pour Déplacer et Faire pivoter), que l'outil tient lui-même et qui
    n'est pas mémorisé.
  - Premières options : portée des booléens (tous les exemplaires / celui-ci) ; copier
    (Déplacer, Faire pivoter). Plus tard : axes (modèle / pièce, §2.3), garder l'outil d'un
    booléen (`keep_cuts` d'OCL), options de pousser/tirer et des usinages.

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
| `crates/model` — `solidsketch-model` | `Document`, `Definition`, `Entity`, ids, chemins d'instance, `Operation` + `apply`, historique d'annulation, fichier JSON versionné, lecture des valeurs tapées (expressions, `@`, listes, points). Aucune dépendance UI ; testable sans affichage. | `kernel`, `serde` |
| `crates/render` — `solidsketch-render` | Pipelines wgpu (faces ombrées, arêtes, grille, axes, surbrillance, aperçus), cache GPU des maillages par solide, dessin d'une liste `DrawItem { mesh, transform, style }` avec une matrice vue-projection. Ne connaît ni egui ni le document. Utilisable hors écran. | `kernel` (pour `Mesh`), `wgpu`, `glam` |
| `crates/tools` — `solidsketch-tools` | Caméra (orbite/pan/zoom, rayon depuis l'écran), scène d'affichage (aplatissement du document en solides placés + cache de maillages), sélection par rayon (CPU, triangle → face B-rep), machines à états des outils, saisie de cote, inférences. Testable sans UI ni GPU. | `model`, `kernel`, `glam` |
| `apps/solidsketch` | eframe : barre d'outils, arborescence, propriétés, barre d'état + VCB, viewport (cible hors écran de `render` affichée comme texture egui), service du noyau (thread natif ou Web Worker), traduction des événements egui en événements d'outils, ouverture/enregistrement. Natif et wasm. | tout ce qui précède, `eframe`, `egui-wgpu` |

## 7 bis. Affichage : la définition comme unité de rendu

Décidé le 2026-11-09. Un composant est un maillage instancié : l'affichage suit le modèle.

**Avec le modèle du §2** (en place) : seule une pièce a de la géométrie (son B-rep final), un
assemblage n'en a pas. L'unité de rendu est donc **le B-rep final d'une pièce** — un buffer par
pièce, dessiné par instanciation avec les matrices de tous ses exemplaires. Modifier une pièce
ne renvoie que ses propres triangles au GPU (fin du risque 2 de la revue). Ci-dessous, « une
définition » se lit donc « une pièce ». **Miroir** : un exemplaire symétrique retourne le sens
des triangles ; le signe du déterminant est passé au shader (`n0.w`), qui en déduit les faces
avant (test `a_mirrored_placement_shows_its_outside`).

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

## 7 ter. Noyau asynchrone

Décidé le 2026-11-09 (questions/réponses). Le noyau ne tourne plus sur le thread de l'UI : un
booléen lourd ne gèle plus l'interface et, dans le navigateur, une panique du noyau (`panic =
abort` en wasm) ne tue plus que le worker.

- **Tâches noyau pures.** Une `Operation` s'applique en deux temps :
  1. `Document::prepare` (thread de l'UI) exécute l'opération avec un *enregistreur* à la
     place du noyau : il valide tout (ids, cycles…), relève les appels au noyau dans un `Job`
     (`kernel::Job` : records d'entrée dédoublonnés + `Task`s — primitive, transformation,
     booléen, affichage) et rend des solides fictifs. Une opération sans appel au noyau
     (renommer, regrouper, déplacer un groupe…) est **finie dès cette étape**, sans aller-retour ;
  2. le `Job` part au service du noyau, qui rend pour chaque solide produit son record **et son
     affichage** (maillage + accroches), d'un seul aller-retour ;
  3. `Document::commit` rejoue l'opération sur le même document en distribuant les résultats
     dans l'ordre ; `History::push` l'enregistre.

  Le document, l'historique et la scène restent sur le thread de l'UI ; le service est sans
  état.
- **Un service, trois exécutions** (`kernel::KernelService` : `submit(id, job)`, `poll()`) :
  `Inline` (synchrone, tests), `Threaded` (un thread, desktop), Web Worker (navigateur, dans
  l'app). Chaque résultat réveille l'UI (`request_repaint`).
- **Pendant le calcul** : une seule opération à la fois ; la navigation et le survol restent
  actifs, les clics d'outils, raccourcis d'édition, annuler / rétablir sont ignorés ; la barre
  d'état dit « Calcul… » et le curseur attend. Échap abandonne : le résultat sera jeté. Les
  actions composées (éclater plusieurs instances) passent par une file interne, une opération à
  la fois. Remplacer une copie par un réseau (`Command::Replace`) rétablit la copie si le
  réseau échoue ou est abandonné.
- **Maillages** : la scène garde le maillage de tout solide encore tenu par l'historique
  (annuler / rétablir ne remaillent pas). Un solide sans maillage (fichier ouvert) est demandé
  au service (une tâche par solide), dessiné en attendant par les arêtes de sa boîte englobante
  (calculée sur le record, sans noyau) ; le document reste utilisable.
- **Web** : deux wasm. `apps/kernel-worker` (noyau seul) tourne dans un Web Worker module
  (`web/kernel-worker.js`) ; le wasm de l'UI ne contient plus SolidCore : `kernel` a une
  feature `engine` (exécution : `solidcore`, `-convert`, `-tess`) ; sans elle il n'expose que
  les types (`Solid`, `Mesh`, `Snaps`, `Job`…) et `solidcore-record`. Messages en
  **MessagePack** (`rmp-serde`, champs nommés : le record a des enums étiquetées, postcard ou
  bincode ne conviennent pas), dans des `ArrayBuffer` transférés. Si le worker plante, son
  travail en cours échoue (message dans la barre d'état), il est relancé et les autres tâches
  en vol lui sont renvoyées.
- **Tester la reprise** : menu **Débogage › Faire planter le noyau** (tâche `Task::Panic`, suivie
  d'une tâche normale qui doit aboutir). Le menu s'active au lancement, dans tout build :
  `?debug` dans l'adresse de la page, `SOLIDSKETCH_DEBUG` dans l'environnement sur desktop.

## 8. Fichier

JSON versionné (`{"format": "solidsketch", "version": 1, …}`) : sérialisation serde du
document, B-rep compris (format record de SolidCore), et ses réglages (`settings`, absent des
premiers fichiers : valeurs par défaut). Dans le navigateur : téléchargement et
chargement de fichier.

**Version 2** (écrite depuis le 2026-11-10) : composants, exemplaires, recettes des pièces
(brut + usinages, avec leur `source`). Le B-rep final n'est pas la source : il est enregistré en
cache (`shape`), présent exactement quand la pièce a des usinages, pour ouvrir sans recalcul.
Les fichiers version 1 sont convertis à l'ouverture (§2.7). **Fichiers de référence** :
`crates/model/tests/files/v1.solidsketch` et `v2.solidsketch`, relus par le test
`reference_files_still_open` — un changement de format qui casserait d'anciens fichiers y
échoue. Ils ne se régénèrent que pour une nouvelle version (`write_reference_files`).

## 9. Chaîne d'outils et dépendances

- Rust **1.97.1** figé par `rust-toolchain.toml` (les crates SolidCore demandent ≥ 1.88), avec
  la cible `wasm32-unknown-unknown`.
- `solidcore` est sur crates.io, mais `solidcore-record`, `-convert` et `-tess` ne le sont pas :
  les quatre viennent du miroir git `tangency-labs/solidcore`, **figés sur un même commit**.
- wasm : pile de 8 Mio (`.cargo/config.toml`) — SolidCore récurse légitimement et wasm n'a pas
  de page de garde. Les paniques du noyau ne sont pas rattrapables en wasm (`panic = abort`) :
  elles sont confinées au Web Worker du noyau (§7 ter).

## 10. Périmètre du prototype v0, par étapes

1. **Socle** : `kernel` (fait, à finaliser), `model` (document, opérations, undo, JSON),
   `render` + app : viewport (orbite/pan/zoom, grille, axes), arborescence, sélection,
   primitives, déplacer/rotation, annuler/rétablir ; build natif + wasm.
2. **Groupes & composants** : créer depuis la sélection, éclater, entrer/sortir d'édition,
   rendre unique, instances multiples. *(Fait, sur l'ancien modèle ; remplacé par le §2.)*
2 bis. **Pièces, assemblages, usinages** (§2) : voir le plan dans `docs/REVUE.md`.
3. **Booléens** : union, différence, intersection, y compris entre contextes (§4).
4. **Pousser/tirer** : sélection d'une face B-rep, décalage (`offset_faces` du noyau, ou
   extrusion + booléen). Le plus risqué côté noyau : en dernier.
