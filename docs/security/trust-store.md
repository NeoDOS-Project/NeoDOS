# Trust store

Los clientes TLS de NeoDOS (`neocurl`, y cualquier consumidor de
`libnet::http_transport`) validan siempre la cadena, el nombre del servidor y
las fechas. Qué **anclas** usan se decide aquí.

## Dos almacenes, a propósito

| Almacén | Dónde | Quién lo escribe | ¿Se puede borrar? |
| --- | --- | --- | --- |
| **Sistema** | `\NeoDOS\System\config\roots.der` (bundle versionado, cabecera `NDTB`) | NeoDev, al construir la imagen desde el store del host | No: es de solo lectura y no lo toca nada en runtime |
| **Usuario** | `\Registry\Machine\Software\NeoDOS\Trust\UserCAs\<sha256-hex>` | `certctl import` | Sí, `certctl remove` |

El almacén de usuario vive en el hive `SOFTWARE`, montado en
`\Registry\Machine\Software` y persistido en
`C:\NeoDOS\System\config\SOFTWARE.hiv` (el hive `SYSTEM` solo sirve rutas
bajo su propio punto de montaje). Mantenerlos separados es lo que garantiza que
borrar una CA importada **nunca** puede afectar a las raíces de la imagen. TLS
usa la unión de los dos, en ese orden, revalidada en cada lectura.

## Alta de una CA

```text
certctl import C:\CA.PEM        # PEM (uno o varios) o DER
certctl list                    # [S] sistema, [U] usuario
certctl show <huella>           # sujeto, emisor, validez, tipo, huella SHA-256
certctl remove <huella>         # solo CAs de usuario
certctl status                  # bundle, anclas utilizables e ignoradas
certctl repair                  # limpia corrupción estructural
```

Reglas de validación (en `libtrust::validate`, con tests de host):

- DER válido y **exacto** (sin bytes trailing), de como máximo
  `MAX_CERTIFICATE_DER` = **8 KiB** (`libtrust::MAX_CERTIFICATE_DER`).
- `basicConstraints.cA == TRUE`.
- `keyUsage`, si está presente, debe permitir `keyCertSign`. Ausente ⇒ se
  acepta (muchas CA raíz no lo llevan).
- Dentro de su ventana de validez, según el reloj fiable (M7).
- Duplicados por SHA-256 (del bundle del sistema o del usuario) ⇒ «ya presente».

**Lo que no se hace:** importar un **certificado de servidor** (hoja, sin
`CA:TRUE`) se rechaza con un mensaje explícito. Confiar en un certificado de
servidor concreto (pinning) es otra función y no está implementada.

## Límite de tamaño

`MAX_CERTIFICATE_DER` es el único límite de un certificado y se aplica en tres
puntos:

1. `validate()` rechaza el DER antes de escribir nada.
2. `user_der()` rechaza un valor del Registry mayor que el límite.
3. `anchor_report()` vuelve a comprobarlo al cargar.

Un valor por debajo del límite se acepta; uno de exactamente 8192 bytes cabe;
uno de 8193 se rechaza con `TrustError::TooLarge` / `StoreValueError::TooLarge`.
No hay aceptación silenciosa de datos truncados: la consulta tipada del Registry
(`RegistryKey::query_value_info`) exige tipo `REG_BINARY`, longitud declarada
coherente y que el payload completo haya cabido en el buffer.

## Persistencia y atomicidad

Una CA = una clave de registro con `Der` (REG_BINARY), `Subject`, `Issuer`,
`NotBefore`, `NotAfter`. `Der` se escribe **el último** y luego se hace `flush`,
así que una importación interrumpida deja como mucho una clave sin `Der`:
`anchor_report()` la ignora y `certctl repair` la borra. El borrado es un
`delete_key` sobre esa clave y un `flush`.

`Der` es la fuente de verdad; `Subject`/`Issuer`/`NotBefore`/`NotAfter` son
metadatos de presentación y no se usan para decidir la confianza.

## Revalidación al recuperar

El Registry es almacenamiento, no una decisión de confianza. En cada lectura
(`libtrust::store::anchor_report(now)`):

- se comprueba que `Der` es `REG_BINARY`, con longitud completa y huella
  SHA-256 igual al nombre de la clave;
- se parsea el DER de nuevo (sin bytes extra);
- se exige `basicConstraints.cA`, `keyUsage.keyCertSign` y ventana de validez;
- una entrada inválida se **omite y se registra**; no hace panic ni impide usar
  las demás raíces válidas.

`certctl status` muestra las anclas utilizables y las ignoradas con su motivo.
`certctl repair` solo elimina corrupción estructural (sin `Der`, tipo
incorrecto, truncado, sobredimensionado o huella que no cuadra). **No** borra
automáticamente una CA caducada, no-CA o con `keyUsage` inválido: eso lo decide
el administrador (puede eliminarla por huella completa).

## Permisos (NT: Cm + VFS)

La protección es **NT**, no bits rwx de UNIX.

### Registry (Cm)

Las mutaciones pasan por `neodos-kernel/src/cm/security.rs`:

- `\Registry\Machine\Software\NeoDOS\Trust` tiene una DACL: `Everyone`
  (`S-1-1-0`) puede leer; SYSTEM (`S-1-5-18`) y Administrators
  (`S-1-5-32-544`) pueden crear, escribir o borrar.
- Se evalúa con `SeAccessCheck` sobre la ruta completa (`Hive::key_path`) en
  `cm_create_key`, `cm_set_value`, `cm_delete_value` y `cm_delete_key`.
- Borrar un ancestro (`Software\NeoDOS`) exige autorización por el borrado
  recursivo.

### Ficheros (VFS)

`SOFTWARE.hiv` y `roots.der` se protegen con una DACL NT persistente:

- `DirEntryV2` lleva un `security_id` en el padding (sigue midiendo 128 bytes).
- La tabla de seguridad de NeoFS guarda `SecurityDescriptor` (owner, group,
  DACL con ACEs); `security_id = 0` usa el descriptor por defecto.
- `ObOpen`, `ReadContent`/`WriteContent`, `FileCreate`, `FileDelete`,
  `VfsRename` y `ObDestroy` de directorios comprueban `SeAccessCheck`.
- El recorrido de directorios exige `EXECUTE` en cada padre.
- Descriptor por defecto de una entrada antigua: Everyone lee/ejecuta; solo
  SYSTEM/Administrators escriben. Por tanto, un usuario ordinario no puede
  abrir `roots.der`/`SOFTWARE.hiv` para escritura, borrarlos ni renombrarlos.
- Un fichero nuevo recibe un descriptor con el token creador como owner.

**Limitación actual:** #767 sigue abierto (todo el userland hereda token admin),
así que el rechazo Ring 3 real no se puede ejercitar todavía. Los tests usan
tokens SYSTEM/Administrators/usuario. Los directorios de perfil de imagen usan
el descriptor por defecto (SYSTEM); para que un usuario cree ficheros en su
perfil hará falta asignarles owner en el alta de perfil (#743/#767).

**Atomicidad Cm:** la ruta, `SeAccessCheck` y la mutación se ejecutan bajo el
mismo `CM_MANAGER`; no hay ventana entre autorizar una celda y modificarla. El
token se obtiene antes del bloqueo (SCHEDULER → CM_MANAGER).

**Empaquetado de `certctl`:** #586 está **cerrada** y cubría NXL, no el listado
NXE. El NeoDev 0.3.0 instalado mantiene una lista fija de NXE que no incluye
`certctl`; el HEAD actual de NeoDev ya lo incluye. Hay que actualizar NeoDev.

## Integración

- `libnet::http_transport::{system_trust(now), system_roots(now)}` leen la unión
  de ambos almacenes; `neocurl` obtiene `now` del reloj fiable antes de cargar
  las anclas y falla cerrado si no lo hay.
- `neocurl` no necesita `--ca` (sigue pudiendo sustituirlos). `--ca` pasa por la
  misma validación de ancla en `libtls` (CA, `keyCertSign`, fechas).
- **NeoCfg**: el módulo *Certificados* consume
  `libtrust::store::{system_entries, user_entries, import_all, remove}` a través
  del seam `CfgPlatform::certificates()`; muestra las anclas del sistema (solo
  lectura), permite importar desde un fichero y borrar solo las CAs de usuario.

## Limitaciones X.509 (reales)

- Sin revocación (OCSP/CRL).
- `nameConstraints` no se aplican.
- `must-staple`/SCT no se comprueban.
- La validación de EKU de intermedios y `nameConstraints` sigue pendiente en
  #772; las anclas y la cadena sí comprueban `keyCertSign`.
- El bundle del sistema es de solo lectura; si se corrompe, `system_bundle()`
  no cae a una lista parcial: se considera ausente.
