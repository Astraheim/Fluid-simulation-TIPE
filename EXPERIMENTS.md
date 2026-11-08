# Bateaux, résultats et classement du TIPE

## Utilisation dans le menu

1. Construire un bateau, puis **Appliquer bateau**. L'organisation **Étagé avec supports** crée des piles croissantes vers la poupe sur trois niveaux de supports pleins ; « Rangées » règle le maximum de niveaux, « Colonnes » les travées, et « Hauteur des gradins » les supports. Il n'y a pas de vide entre conteneurs superposés.
2. Ouvrir **Bateaux sauvegardés / classement**, donner un nom, puis **Sauvegarder bateau**. Le dossier `results/boats/` conserve les configurations même après fermeture.
3. **Copier caractéristiques** produit un code JSON copiable. Coller ce code dans le champ et cliquer sur **Charger le code** restaure les réglages et la géométrie réellement appliquée, y compris les bateaux aléatoires et les murs dessinés. Le champ de code est repliable, sa hauteur est limitée avec un défilement interne, et il se replie automatiquement après copie ou chargement. Le bateau redémarre à zéro, en pause. Ce n'est pas une sauvegarde du champ de vitesse en cours.
4. Régler les pas de stabilisation et de mesure, puis **Mesurer et classer**. L'essai repart de zéro, accumule les statistiques en mémoire et s'arrête automatiquement. Une modification physique interrompt l'essai et l'exclut du classement ; une pause ou un changement de couleur ne l'invalide pas.
5. Les essais terminés apparaissent du meilleur au moins bon, avec **Charger** et **Copier**. Changer les poids recalcule immédiatement le classement. **Enregistrer ce classement / ces poids** conserve ce choix dans `results/ranking.json` ; la fin d'un essai enregistre également le classement courant.

Le menu occupe sa propre zone à gauche et la simulation carrée s'ajuste à la place disponible à droite, y compris après agrandissement. Le dessin à la souris utilise les coordonnées de cette zone : clic gauche maintenu pour le tracé libre, ou glisser-relâcher avec le mode ligne droite. Les gestes sont traités par le widget de simulation ; les clics dans le menu ne dessinent pas de murs.

## Score et comparabilité

Le meilleur score est le plus grand :

`score = poids_capacité × N/Nref − poids_traînée × |moyenne(Fx)|/Dref − poids_protections × P/Pref`

- `N` : nombre de conteneurs effectivement présents dans la grille, identifiés séparément. En vue de côté ce n'est pas une capacité en TEU d'un navire 3D.
- `P` : nombre de cellules de proue, de poupe, de carénage et de supports ajoutés. C'est une approximation de surface de matière en 2D, **pas un coût en euros**, ni un calcul de masse/épaisseur de tôle. Les plateformes historiques du bateau biblio restent dans sa coque fixe.
- Références initiales : 65 conteneurs, une traînée numérique de 1, 1 000 cellules d'aménagement. Poids initiaux égaux à 1. Ce sont des choix de normalisation modifiables, pas un optimum économique démontré.
- Le couple et les autres mesures restent disponibles indépendamment du score.

Les classements sont séparés **uniquement par vitesse d'entrée et dimensions du corps central (longueur × hauteur dans cette vue 2D)**. Proue, poupe, protections, chargement, ligne d'eau, durée, autres réglages et versions du programme/dépendances ne créent pas de groupes supplémentaires. Les anciens résultats de l'interface sont regroupés automatiquement à la lecture, sans les modifier. Les dimensions appliquées sont enregistrées explicitement ; pour les anciens fichiers on utilise les dimensions des curseurs (ou celles du modèle biblio). Si un ancien fichier ne contient ni dimensions ni curseurs, seule l'enveloppe des cellules de coque est disponible pour déterminer sa taille.

Le bouton **Classements** ouvre une fenêtre séparée : sélectionner un groupe nommé « Coque longueur × hauteur · vitesse », puis utiliser **Charger** ou **Copier** dans le tableau. Les poids sont réglables dans cette fenêtre. Les autres conditions restent enregistrées pour interpréter les résultats, mais ne bloquent plus les comparaisons.

Un essai « completed » a terminé sa fenêtre, **cela ne prouve ni convergence ni validité physique**. Le résidu est conservé dans la synthèse des résultats. Vérifier les historiques, prolonger la stabilisation et comparer plusieurs fenêtres avant d'interpréter un classement. L'écart type décrit la fluctuation des mesures ; il n'est pas l'incertitude sur la moyenne de données corrélées.

## Fichiers d'un essai

Chaque essai crée un dossier unique `results/run_…/` sans écraser les précédents :

| Fichier | Contenu |
|---|---|
| `config.json` | Réglages et géométrie exacte rechargeable, version du format, grille, pas de temps et origine du code |
| `../manifests/build_….json` (partagé) | Une seule copie des sources techniques, dépendances et conventions par contenu distinct |
| `summary.json` | Synthèse unique : moyennes, écarts types, statut, nombre de pas, temps, quantités de murs et score avec sa formule et ses poids ; aucune copie de la configuration ni de la géométrie |

Les statistiques comprennent : Fx, Fy, couple, traînée des conteneurs, maximum absolu du potentiel de pression, vitesse maximale au centre des cellules, résidu absolu/relatif du Poisson, itérations, débits gauche/droite, flux sortant net sur les quatre côtés, durée du pas (injection + simulation, sans rendu ni écriture), divergence finale maximale, Cx/Cy indicatifs, conteneurs et quantités de cellules solides.

`Cx_proxy` et `Cy_proxy` valent `F / (0,5 rho_air U² Lref)` avec `Lref = 1` (longueur normalisée du domaine, profondeur unitaire). À vitesse nulle, ces coefficients ne sont pas calculés. Les forces reprennent le modèle historique du programme : potentiel de pression et terme de traînée empirique. **Ces coefficients ne sont pas des coefficients aérodynamiques SI validés.** Le potentiel est celui de la projection avant advection ; les vitesses sont celles de la fin du pas. Les axes sont x vers la droite et y vers le bas. Le couple est calculé autour du centre fixe du domaine, avec bras de levier divisés par N ; c'est un moment dans la vue de côté, pas un moment de lacet 3D.

La géométrie et les étiquettes évitent de compter la boîte d'injection ou le domaine bloqué sous l'eau dans les forces du bateau. Le solveur et sa boîte d'injection sont conservés ; seul le diagnostic du résidu a été ajouté. Les maxima de champ et le bilan de débit concernent tout le domaine fluide.

Fermer normalement pendant un essai écrit la synthèse des mesures accumulées avec le statut `interrupted`. Un arrêt brutal peut laisser seulement la synthèse initiale `running` : les mesures en mémoire sont alors perdues et l’essai n’est pas classé. Il n’y a plus d’écriture de fichier à chaque pas. Une valeur non finie exclut également l'essai. Les fichiers sont locaux au dossier de lancement, pas synchronisés automatiquement par Git. Les anciens essais restent lisibles ; leurs anciens fichiers ne sont pas supprimés.

Le score aux poids par défaut se trouve dans `summary.json`, sous `score`. Le classement global `ranking.json` conserve les poids choisis dans le menu. Il reste **deux fichiers par nouvel essai** : `config.json` et `summary.json`. Le champ `manifest` de la synthèse pointe vers le fichier technique partagé (chemin relatif au dossier de l’essai). Pour déplacer les résultats avec leur provenance, copier le dossier `results` entier.

### À quoi sert le manifest ?

`config.json` décrit le bateau et les réglages. `manifest.json` décrit le **programme qui a réalisé le calcul** : empreinte des sources, compilateur et architecture, constantes non réglables dans le menu, versions des dépendances, modèle de forces, unités/conventions et état initial. Il permet de comprendre pourquoi un même bateau pourrait donner un résultat différent après une modification du code. Il est désormais enregistré dans `results/manifests/` une seule fois par contenu technique distinct et réutilisé par tous les essais concernés. Il ne participe jamais à la clé de classement. Une mise à jour de dépendances peut donc créer une nouvelle fiche technique, mais **pas un nouveau classement**. Les anciens fichiers `manifest.json` sont laissés intacts. Cette fiche n’est pas nécessaire pour charger un bateau dans le menu.

## Campagne sans interface

Depuis la racine du projet :

```sh
cargo run --release -- --init-campaign campaign.json
cargo run --release -- --batch campaign.json
# Rejouer une configuration sauvegardée :
cargo run --release -- --run results/run_.../config.json results
```

La campagne initiale compare trois chargements sur la même coque biblio, chacun avec et sans carénage avant (six bateaux), à deux vitesses numériques (0,6 et 1), avec 300 pas de stabilisation et 600 de mesure. Modifier `speeds`, `warmup`, `measure` et la liste `boats` dans le JSON, ou y copier des configurations enregistrées dans le menu. Le mode batch écrit les mêmes fichiers que l'interface et ses résultats sont rechargeables via **Actualiser les résultats**. Les durées par défaut sont un point de départ à vérifier, pas un critère automatique de convergence.

## Choix de référence et publications

- Deng, Song et Ren (2022), *Investigation on the effect of container configurations and forecastle fairings on wind resistance and aerodynamic performance of large container ships*, DOI [10.1080/19942060.2022.2086177](https://doi.org/10.1080/19942060.2022.2086177). Comparaisons de chargements et de carénages à coque fixe : longueur 144,80 m, largeur 23,40 m, franc-bord 8 m, maquette 1:60, essais à 15 et 25 m/s. La présente campagne reprend le principe de comparaison et le rapport de vitesses 15/25, **pas une conversion du curseur en m/s**. La silhouette biblio est estimée d'après la figure fournie, avec carène inventée : ce n'est pas une reproduction à l'échelle du modèle expérimental.
- ITTC (2024), [Guideline on the CFD-based Determination of Wind Resistance Coefficients](https://www.ittc.info/media/11962/75-03-02-05.pdf), 7.5-03-02-05 : conventions de vitesse et surface de référence, cohérence de géométrie, contrôle du maillage et de la convergence. Une étude 2D sans modèle de turbulence ni viscosité ne reproduit pas ces calculs 3D ; conserver les grandeurs brutes et les diagnostics permet de préparer ces vérifications sans masquer les limites actuelles.

## Bateau aléatoire et extrémités de coque

Le bouton **Bateau aléatoire (coque conservée)** du générateur tire les réglages de géométrie et les options (disposition et nombre de conteneurs, supports, proue, poupe et carénages). La longueur et la hauteur du corps central, ainsi que la largeur et la hauteur des conteneurs, restent celles choisies manuellement. Le franc-bord, et donc la hauteur d’eau à dimensions de coque constantes, ainsi que les réglages du vent sont conservés. La proue et la poupe sont toujours activées lors du tirage, avec une longueur maximale de 100 cellules (réduite si nécessaire selon la place disponible). La hauteur relative des carénages est limitée à 1,1, dans les curseurs comme dans le tirage. Les curseurs reflètent le tirage et le bateau est appliqué immédiatement. Les piles sont limitées à la place disponible ; les réglages de vent et de mesure ne sont pas tirés au hasard. Le mode biblio est désactivé pour laisser ces réglages agir.

La proue et la poupe sont maintenant comptées comme protections dans le coût. Leurs formes peuvent varier dans un même classement, à corps central et autres conditions identiques. Leur géométrie et leurs forces restent intégralement présentes dans la simulation. Le tirage conserve désormais le franc-bord et les dimensions des conteneurs : les variantes ainsi générées peuvent être comparées avec le même protocole.

### Générer, tester et classer en un clic

Dans **Bateaux sauvegardés / classement**, le bouton **Tester et classer un bateau aléatoire** génère et applique un bateau avec les dimensions de coque et de conteneurs sélectionnées, conserve le franc-bord et le vent, lui attribue un nom unique et démarre immédiatement l’essai depuis zéro. Il utilise les nombres de pas de stabilisation et de mesure du menu. À la fin, la synthèse est sauvegardée et le classement est actualisé automatiquement. Le bouton n’est proposé que lorsqu’aucun essai n’est en cours.

### Chargement minimum des bateaux aléatoires

Les deux boutons aléatoires imposent au moins **50 conteneurs**. Le nombre est calculé selon la disposition (grille, pyramide, quinconce ou étagée), avec un espacement adapté à la place disponible et des dimensions de conteneurs conservées. Seules les dispositions permettant ce chargement sont tirées. Si aucune ne convient, un message explique l’impossibilité et le bateau courant reste inchangé ; aucun essai aléatoire ne démarre.


## Présélection des essais aléatoires

Le bouton **Tester et classer un bateau aléatoire** conserve le minimum de 50 conteneurs et cherche jusqu’à 64 géométries avant de lancer le solveur. Dans le groupe correspondant aux dimensions et à la vitesse courantes, il compare le meilleur score mesuré au score maximal possible du candidat :

`score maximal possible = poids_capacité × N/Nref − poids_protections × P/Pref`

La pénalité de traînée est au mieux nulle. Si ce majorant ne dépasse pas le meilleur score enregistré, le candidat est éliminé sans simulation. Sinon, son aérodynamique doit encore être mesurée. Ce filtre utilise les poids actuels et les quantités réellement présentes dans la grille ; il ne prouve pas le réalisme physique et ne suppose pas qu’une protection importante soit forcément inutile. Sans résultat de référence dans le groupe, un candidat ayant au moins 50 conteneurs peut être testé. Si aucun des 64 candidats ne passe, aucun essai n’est lancé et le bateau précédent est conservé. Le bouton de génération simple et les essais manuels restent disponibles sans ce filtre de score.

## Conteneurs : critères exploratoires (novembre 2026)

- Limite de modèle : **10 étages**, contrôlée sur la géométrie enregistrée,
  y compris les imports et les anciens résultats. Les tirages et le curseur
  respectent cette limite ; les essais au-dessus sont exclus. Ce n'est pas une
  résistance certifiée : certains navires atteignent 11–12 étages avec arrimage.
- Deux bonus réglables (0,1 chacun par défaut) :
  `abri = 1 / (1 + moyenne_des_normes_des_forces_par_conteneur / Dref)` et
  `accès = fraction des conteneurs bordant une bande verticale libre`.
  La bande représente 10 % de la largeur discrétisée du conteneur et doit être
  libre depuis le sommet du chargement jusqu'au bas du conteneur. Cela ne prouve
  ni l'accès aux portes ni le passage d'une grue. La protection mutuelle entre
  conteneurs compte aussi comme abri : aucun bonus arbitraire pour un carénage.
  Les anciennes mesures sans effort individuel n'obtiennent pas le bonus d'abri :
  refaire leurs essais pour comparer ce critère. Le majorant de présélection
  inclut les bonus afin de ne pas éliminer un candidat pouvant gagner.
- Stabilité : forces de chaque cellule intégrées en moments autour des deux
  arêtes inférieures de chaque conteneur ; poids appliqué au centre.
  Rejet si moment déstabilisant >= `m*g*b/2`, ou portance >= `m*g`.
  Une force horizontale à mi-hauteur donne `Fcrit = m*g*b/h`.
  Le contrôle porte sur chaque pas, stabilisation comprise, pas sur une moyenne.
- **Les forces historiques ne sont pas SI.** `Calibration N/unité = 0` signifie
  « non évalué », jamais « sûr ». Saisir une conversion établie par calibration
  avant d'activer l'exclusion physique. Une simple multiplication ne valide pas
  le solveur. Le classement indique le statut et le résumé garde le pic du ratio.
  Par défaut : 15 000 kg (hypothèse, pas une moyenne statistique), base 6,058 m
  dans le sens du vent longitudinal, hauteur 2,591 m, conteneur standard 20 pieds.
  Pour un vent transversal utiliser la largeur de base 2,438 m. Modifier ces
  dimensions physiques ne redimensionne pas la grille.
- Modèle de boîte individuelle sans arrimage, glissement, transfert d'efforts
  entre conteneurs, vérification des appuis, compression de pile ou mouvement du
  navire. Ce contrôle n'est donc pas une validation de sécurité maritime.
- Les paramètres sont enregistrés avec chaque bateau ; ils ne créent pas de
  nouveaux groupes : seuls coque et vitesse continuent à séparer les classements.

Références consultées :
- Hapag-Lloyd, 20' Standard : exemple de tare 2 350 kg et masse brute maximale
  30 480 kg (la masse chargée dépend du contenu).
  https://www.hapag-lloyd.com/en/services-information/cargo-fleet/container/20-standard.html
- Présentation CMA Ships hébergée par l'OMI : capacités de navires avec 11–12
  étages, à distinguer d'une limite universelle de gerbage.
  https://wwwcdn.imo.org/localresources/en/OurWork/Environment/Documents/Symposium%202021/Presentations/First%20day%20-%20Blocks%201%20and%202/Symposium%20presentations%20-%20First%20Day/Block%201.1%20-%20Xavier%20Leclercq_CMA%20SHIPS%20operating%20LNG-fuelled%20containerships.pdf

Le classement recharge le `config.json` du même dossier pour retrouver le bateau. Conserver les deux fichiers ensemble lors du déplacement d’un essai. Les anciennes synthèses avec configuration intégrée restent lisibles ; le chemin du dossier est déterminé à la lecture.
