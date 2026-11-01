# Ketchup Server

Fondation PHP pour la connexion Google, la session utilisateur et la déconnexion. Aucun partage de projet ni édition collaborative. Le client Rust reste indépendant.

## Démarrage

PHP 8.2 ou supérieur avec les extensions curl et JSON. Aucune installation Composer nécessaire.

```sh
cd /home/dcoulombe/dev/ketchup/server
cp config.example.php config.local.php
# Renseigner client_id et client_secret dans config.local.php.
php -S localhost:8081 -t public public/index.php
```

Le port 8081 laisse le port 8080 disponible pour le client KetchUp.

Ouvrir http://localhost:8081. Sans identifiants, la page fonctionne et la connexion affiche une erreur explicite. Il n’existe aucun mode de connexion simulée dans le serveur.

## Configuration Google Cloud

1. Créer ou choisir un projet Google Cloud et configurer l’écran de consentement Google Auth Platform (nom, audience, coordonnées). En mode test, ajouter les comptes autorisés comme utilisateurs de test.
2. Créer un client OAuth **Application Web**.
3. Ajouter exactement l’URI de redirection autorisée `http://localhost:8081/auth/google/callback`.
4. Reporter l’ID et le secret du client dans `config.local.php`, ignoré par Git. Ne jamais exposer ce fichier via le serveur Web.

La configuration réelle est le dernier prérequis pour tester une connexion de bout en bout avec Google. Aucun identifiant n’est fourni dans le dépôt.

En production, définir une URI de rappel HTTPS et enregistrer la même URI dans Google Cloud. Configurer le serveur Web avec **public/** comme seule racine et router les requêtes vers `public/index.php`. Le serveur intégré PHP est destiné au développement. La session PHP doit disposer d’un stockage privé et persistant ; plusieurs instances doivent partager ce stockage. Le cookie Secure est activé selon le schéma HTTPS de l’URI configurée, y compris derrière un proxy qui termine TLS. Ne jamais servir cette configuration HTTPS via HTTP public.

## Routes

- `GET /` : état connecté et formulaire de déconnexion.
- `GET /auth/google` : début du flux OAuth.
- `GET /auth/google/callback` : rappel Google.
- `GET /api/session` : JSON `{ "authenticated": false, "user": null }`, ou profil de la session courante. Aucun CORS public.
- `POST /logout` : déconnexion locale, avec jeton CSRF du formulaire ; ne déconnecte pas le compte Google global.

Le flux utilise un code d’autorisation, PKCE S256 et un state aléatoire lié à la session, consommé une seule fois et expirant après dix minutes. L’identité provient de l’endpoint HTTPS Google UserInfo avec le jeton obtenu côté serveur ; aucun ID token non vérifié n’est utilisé. Le `sub` Google est l’identifiant stable, pas l’adresse courriel. Les jetons Google ne sont pas conservés. L’ID de session est renouvelé au début et après la connexion. Les cookies sont HttpOnly et SameSite=Lax ; les sessions authentifiées expirent après huit heures. Les réponses ne sont pas mises en cache et le HTML est échappé. Les erreurs ne révèlent ni réponse brute Google ni secret.

## Tests

```sh
php tests/run.php
php tests/http.php
# Ou : composer test
find src public tests -name '*.php' -exec php -l {} \;
```

Les tests injectent le transport fournisseur exclusivement dans la classe d’authentification : paramètres OAuth et PKCE, échange de code, profil, state invalide/expiré/à usage unique, refus, code absent, défaillance réseau, réponses invalides, session, expiration et CSRF de déconnexion. Ils ne nécessitent pas d’identifiants ni d’accès à Google. Le test HTTP ouvre temporairement un port local et vérifie les routes, le cookie de session et les rejets de requêtes invalides. La validation réelle auprès de Google nécessite le paramétrage ci-dessus.

Référence : [OpenID Connect Google, flux serveur](https://developers.google.com/identity/openid-connect/openid-connect).
