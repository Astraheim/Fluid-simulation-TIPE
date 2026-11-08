# Branche expérimentale : convergence de pression

Branche `experiment/pressure-convergence`, basée sur `main` à `2da7038`.
Aucune fusion dans main. Les résultats de cette branche ne doivent pas être mélangés avec les anciens classements.

## Changements

- Gauss-Seidel plafonné à 20 passages remplacé par un gradient conjugué préconditionné (Cholesky incomplet modifié, MIC(0)), en f64, sur le même opérateur symétrique et les mêmes poids de face.
- Initialisation par la pression précédente, vérification du résidu réel et plafond de 2 000 itérations.
- Tolérance liée à la divergence : `1e-3 + 1e-4 × divergence_max_avant`. Avec `b=-h² div(u*)`, la tolérance de résidu vaut `h²` fois cette tolérance de divergence. La convergence est vérifiée sur les vitesses effectivement stockées en f32, pas seulement sur la solution linéaire.
- Pression définie à une constante près : chaque composante fluide connexe est traitée séparément. La moyenne de son second membre est retirée pour résoudre la partie compatible ; cette correction est enregistrée comme divergence incompatible. Le résidu du problème ORIGINAL reste enregistré. Un problème incompatible au-delà de la tolérance n'est jamais déclaré convergé.
- Ordre du pas : conditions aux limites, advection des vitesses, forces/limitation éventuelle, conditions aux limites, projection, transport de densité/phase. Les vitesses et les flux imposés ne sont plus réécrits après projection.
- Diagnostics avant/après/fin de pas dans l'interface et les statistiques. Un essai dont au moins une mesure a une projection non convergée est sauvegardé avec `non_converged_projection`, hors classement. Les échecs pendant la stabilisation sont visibles mais ne suffisent pas à exclure une fenêtre de mesure ensuite entièrement convergée.

## Vérification reproductible sans fenêtre

Depuis la racine du dépôt :

```sh
cargo test --release --manifest-path validation/pressure/Cargo.toml -- --nocapture
```

Le harnais inclut directement les modules de l'application, sans dépendances de fenêtre. 34 tests réussis : opérateur pondéré, composantes déconnectées, flux incompatibles, divergence non nulle, maintien de la divergence après transport, traceur constant, bateau complet, exclusion des résultats non convergés et tests existants.

Mesures ponctuelles en release, grille N=510 (les durées dépendent de la machine et ne constituent pas un benchmark rigoureux) :

| Cas | Divergence avant | Ancien après | Nouveau après | Itérations nouvelles |
|---|---:|---:|---:|---:|
| Poche fermée 64 × 64, vitesse sinusoïdale | 35,0343 | 57,2490 (20 passages) | 0,00439 | 107 |
| Premier pas sur bateau biblio, entrée 1 | 510 | non mesuré | 0,04954 | 655 |

La tolérance du deuxième cas vaut 0,052 : le test est convergé à la précision demandée, sans prétendre annuler exactement la divergence. Le pas sur bateau a pris environ 2 s ici. Pour une interface fluide il faudra optimiser (meilleur préconditionnement, multigrille, réutilisation des structures) et mesurer plusieurs pas.

## Limites à examiner avant fusion

- Les conditions de pression restent de Neumann. La branche DETECTE les flux incompatibles, elle ne crée pas de condition de pression de sortie. Les domaines ouverts et scénarios avec sources nécessitent encore une étude dédiée de leurs frontières.
- La conservation du traceur constant est testée ; cela NE prouve PAS la conservation globale d'une densité non uniforme. L'advection semi-lagrangienne n'a pas été remplacée par un schéma conservatif. Mesurer les variations de masse, l'injection et les flux séparément.
- Tests sur la configuration actuelle (`ENABLE_WATER=false`). Le graphe pondéré est testé, mais une campagne bi-fluide complète reste à faire.
- L'application graphique complète n'a pas été compilée/exécutée ici : l'environnement manque de dépendances système graphiques. Le solveur, la simulation et les essais sont compilés par le harnais.
- Les pressions et forces sont modifiées : refaire les expériences, sans réutiliser les anciens scores comme comparaison physique directe.

Références : notes de Bridson et Müller-Fischer, https://www.cs.ubc.ca/~rbridson/fluidsimulation/fluids_notes.pdf (résidu de Poisson et divergence résiduelle).

## Accélération du solveur (8 novembre 2026)

Le préconditionneur diagonal initial est remplacé par MIC(0), avec tau=0,97 et un garde-fou sur les pivots. Le stockage creux est regroupé dans des tableaux contigus (CSR), y compris les facteurs triangulaires, pour éviter les allocations dispersées et les branches répétées dans les itérations. Le stencil reste le même, les calculs restent en f64, la grille reste N=510 et les tolérances restent inchangées. Le préconditionneur est destiné au graphe cartésien à cinq points de cette application.

Benchmark de calcul **sans rendu**, en release, ancien solveur au commit `3f43729` contre cette version, sur la même machine : 2 bateaux × 3 répétitions indépendantes × 3 pas = 18 pas pour chaque version. Exécutions séquentielles, répertoires de compilation distincts. Les données brutes sont dans `speed-benchmark.csv`.

| Calcul chronométré | Avant | Après | Accélération |
|---|---:|---:|---:|
| Premier pas, moyenne de 6 démarrages | 2 035 ms | 905 ms | ×2,25 |
| Pas suivants, moyenne de 12 pas | 904 ms | 737 ms | ×1,23 |
| Ensemble des 18 pas | 1 281 ms | 793 ms | ×1,62 |

Les 36 pas satisfont le critère de convergence. Les solutions ne sont pas bit à bit identiques : elles sont arrêtées au même seuil de résidu et vérifiées par la même tolérance de divergence. Le gain n'est donc **pas ×100**, ni constant selon le pas. Le chronométrage exclut le rendu, l'enregistrement des forces/statistiques et la préparation initiale de la géométrie. Il ne garantit pas le même gain sur la frame graphique complète ni sur un autre ordinateur.

Tests supplémentaires : symétrie/positivité du préconditionneur et solution manufacturée sur grille pondérée avec obstacle. Total : 34 tests réussis ; le benchmark est ignoré par défaut, à lancer explicitement :

```sh
cargo test --release --manifest-path validation/pressure/Cargo.toml benchmark_boat_steps -- --ignored --nocapture --test-threads=1
```

Pour comparer deux versions, utiliser des répertoires `CARGO_TARGET_DIR` distincts pour éviter de réutiliser le binaire d'un autre checkout. Pour viser des gains beaucoup plus grands à résolution et précision égales, il faudra évaluer une autre méthode (par exemple multigrille) et éventuellement un portage GPU. Aucun gain correspondant n'a été démontré ici.
