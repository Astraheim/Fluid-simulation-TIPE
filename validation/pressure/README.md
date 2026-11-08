# Branche expérimentale : convergence de pression

Branche `experiment/pressure-convergence`, basée sur `main` à `2da7038`.
Aucune fusion dans main. Les résultats de cette branche ne doivent pas être mélangés avec les anciens classements.

## Changements

- Gauss-Seidel plafonné à 20 passages remplacé par un gradient conjugué préconditionné (Jacobi), en f64, sur le même opérateur symétrique et les mêmes poids de face.
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

Le harnais inclut directement les modules de l'application, sans dépendances de fenêtre. 32 tests réussis : opérateur pondéré, composantes déconnectées, flux incompatibles, divergence non nulle, maintien de la divergence après transport, traceur constant, bateau complet, exclusion des résultats non convergés et tests existants.

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
