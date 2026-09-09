# Hébergement COM/OLE/ActiveX

## Création et interfaces

Le thread UI appelle `OleInitialize` (STA), puis crée la fenêtre Win32. La boucle
de messages reste sur ce thread jusqu'à la destruction du contrôle et de la vue,
avant `OleUninitialize`. Le contrôle est toujours in-process.

Ordre de détection des coclasses enregistrées :

| Coclass | CLSID | Génération Windows |
| --- | --- | --- |
| MsRdpClient8NotSafeForScripting | A3BC03A0-041D-42E3-AD22-882B7865C9C5 | Windows 8 / Server 2012 |
| MsRdpClient7NotSafeForScripting | 54D38BF7-B1EF-4479-9674-1BD6EA465258 | Windows 7 / Server 2008 R2 |

L'échec d'activation COM ou l'absence de NonScriptable5 sur la première classe
fait essayer la seconde. Aucune
classe Windows 10/11 n'est requise. Le suffixe NotSafeForScripting désigne
l'interface destinée à un hôte natif de confiance ; aucun script n'est exécuté.

Le conteneur est une véritable fenêtre `WS_CHILD`, avec une classe à fond noir.
Le code obtient `IOleObject`, `IOleInPlaceObject` et `IDispatch`, installe le site
avec `SetClientSite`, initialise `IPersistStreamInit` lorsqu'il est disponible,
appelle `OleSetContainedObject`, puis `DoVerb(OLEIVERB_INPLACEACTIVATE)`.
`SetObjectRects` synchronise le rectangle client et le rectangle de clipping.

Interfaces implémentées par le conteneur, avec les macros windows-rs :

| Interface | Fonction |
| --- | --- |
| IOleClientSite | Site client, notifications d'affichage, absence de persistance/document |
| IOleInPlaceSite / IOleWindow | HWND enfant, activation in-place, rectangles et contexte |
| IOleInPlaceFrame / IOleInPlaceUIWindow | Cadre supérieur, sans menus/barres fusionnés |
| IOleControlSite | Notifications de focus/activation, refus de propriété interactive |
| IDispatch ambiant | UserMode=true, UIDead=false ; propriétés non gérées refusées |

`GetWindowContext` renvoie le HWND de la fenêtre supérieure pour le cadre et les
rectangles relatifs à la fenêtre enfant pour la zone d'affichage. Aucun ATL ni
composant .NET d'hébergement n'est utilisé.

## Propriétés et interfaces RDP

Les propriétés Automation sont résolues par `IDispatch::GetIDsOfNames/Invoke`,
sans dépendre des numéros de propriétés d'une version récente. AdvancedSettings8
est essayé puis AdvancedSettings7. Les paramètres de sécurité ne sont jamais
ignorés à la suite d'une erreur HRESULT ; SmartSizing est optionnel.

Les interfaces non Automation sont déclarées dans `interfaces.rs`, avec leur
IID et la chaîne complète de vtables héritées issue de la type library MsTscAx :
IMsTscNonScriptable puis IMsRdpClientNonScriptable à NonScriptable5. Le code fait
un QueryInterface explicite pour chaque niveau utilisé ; il ne calcule pas
d'offset de vtable à la volée.

NonScriptable5 est disponible dès Windows 7 et nécessaire à la propriété
`AllowPromptingForCredentials`. Son absence ne fait pas quitter le shell :
elle interdit la connexion et déclenche l'écran Error/retry. La sécurité ne
dépend donc pas d'un fallback qui réactiverait les dialogues.

Le mot de passe est fourni uniquement via `IMsTscNonScriptable::put_ClearTextPassword`.
Une allocation BSTR privée possède son pointeur brut, n'est pas clonée, est
effacée par écritures volatiles puis libérée par l'allocateur BSTR. Les wrappers
VARIANT possèdent leurs BSTR/interfaces et appellent VariantClear à leur destruction.
Les descriptions d'exception Automation ne sont ni demandées ni journalisées.

## Événements

`IConnectionPointContainer::FindConnectionPoint` utilise l'IID
`336D5562-EFA8-482E-8CB3-C5C0FC7A7DB6` (IMsTscAxEvents). Le sink expose réellement
cette dispinterface, pas seulement un IDispatch générique. Advise conserve un
cookie et une référence du sink jusqu'à Unadvise.

| DISPID | Événement | Traitement |
| --- | --- | --- |
| 1 | OnConnecting | Journalisation, contrôle toujours caché |
| 2 | OnConnected | Transport établi ; aucun changement de visibilité |
| 3 | OnLoginComplete | Seule autorisation d'afficher la session |
| 4 | OnDisconnected | Masquage synchrone, code de raison, retry |
| 10 / 11 | OnFatalError / OnWarning | Masquage et retour à l'écran d'échec |
| 15 | OnConfirmClose | Retour VARIANT_BOOL true : fermeture sans confirmation |
| 16 | OnReceivedTSPublicKey | Refus de validation manuelle de clé |
| 17 / 34 | OnAutoReconnecting / OnAutoReconnecting2 | Masquage, arrêt de la reconnexion native |
| 18 / 22 | OnAuthenticationWarningDisplayed / OnLogonError | Refus d'interaction et retour à Error |
| 33 | OnAutoReconnected | Événement inattendu : nouvelle tentative contrôlée |
| 5 / 8 | Demande/entrée fullscreen natif | Refus, retour à Error |

Les paramètres VARIANT des événements sont empruntés uniquement pendant Invoke.
Le code vérifie leur type et leur pointeur avant de modifier les paramètres
BYREF de confirmation/annulation. Aucun pointeur d'argument n'est conservé.

Le sink cache d'abord la fenêtre enfant et invalide/repeint le parent. Il dépose
ensuite une petite valeur d'événement et sa génération dans une file, puis poste
WM_APP. Il n'appelle ni Connect, ni Disconnect, ni destruction COM dans Invoke.
L'orchestrateur vide la file après le retour du message, sans garder d'emprunt
sur cette file pendant des appels COM. Un échec invalide immédiatement la
possibilité d'afficher un OnLoginComplete déjà en attente.

## Réentrance, fermeture et durée de vie

L'ancien pointeur vers un état mutable global a été remplacé par un contexte
de fenêtre partagé : les procédures Win32 ne peuvent modifier que des Cell ou
emprunter la vue avec try_borrow_mut. App possède le client séparément, hors des
callbacks. La présence de Rc/PhantomData<Rc<()>> interdit de déplacer l'ActiveX
sur un autre thread Rust. Tous les appels ont lieu dans l'appartement UI.

Destruction : autorisation de visibilité retirée, Unadvise, Hide/Disconnect,
InPlaceDeactivate, Close(OLECLOSE_NOSAVE), SetClientSite(None), destruction du
HWND enfant, libération des interfaces COM. La fenêtre supérieure est détruite
après le client ; OLE est désinitialisé en dernier. Les erreurs d'un ancien
contrôle sont ignorées par génération et ne peuvent déconnecter le nouveau.

Le hook CBT du thread UI empêche la création des boîtes natives #32770 durant
la vie du contrôle. Son pointeur de contexte est une Weak, pas une adresse
d'objet libéré. Le hook est retiré à la destruction. Cette défense complète les
propriétés anti-prompt ; elle ne constitue pas une désactivation de validation TLS.

## Portée de validation

Le test local crée le vrai contrôle, configure ses propriétés, injecte un secret
synthétique, vérifie l'activation/resize et le refus d'un dialogue natif. Un autre
test appelle réellement Connect sur un port TCP local fermé et pompe les messages
jusqu'à OnDisconnected. Il garde le cadre de test hors écran et le contrôle caché.

Une ouverture de session réelle, l'absence de dialogues pour tous les scénarios
de sécurité et le fonctionnement sous MultiPoint Server 2012 restent à vérifier
avec l'environnement cible. OnLoginComplete est un signal de fin de logon, pas
une garantie que le contenu applicatif distant a fini de se dessiner.
