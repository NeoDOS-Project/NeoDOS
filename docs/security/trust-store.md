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
usa la unión de los dos (`libtrust::store::anchors()`), en ese orden.

## Alta de una CA

```text
certctl import C:\CA.PEM        # PEM (uno o varios) o DER
certctl list                    # [S] sistema, [U] usuario
certctl show <huella>           # sujeto, emisor, validez, tipo, huella SHA-256
certctl remove <huella>         # solo CAs de usuario
certctl status                  # bundle: versión, fecha de build, nº de anclas
certctl repair                  # limpia claves de importaciones interrumpidas
```

Reglas de validación (en `libtrust::validate`, con tests de host):

- DER válido (o PEM que contenga `CERTIFICATE`; los bloques `PRIVATE KEY` se
  ignoran y nunca se guardan).
- `basicConstraints.cA == TRUE`.
- `keyUsage`, si está presente, debe permitir `keyCertSign`. Ausente ⇒ se acepta
  (muchas CA raíz no lo llevan).
- Dentro de su ventana de validez, según el reloj fiable (M7).
- Duplicados por SHA-256 (del bundle del sistema o del usuario) ⇒ «ya presente».

**Lo que no se hace:** importar un **certificado de servidor** (hoja, sin
`CA:TRUE`) se rechaza con un mensaje explícito. Confiar en un certificado de
servidor concreto (pinning) es otra función y no está implementada.

## Persistencia y atomicidad

Una CA = una clave de registro con `Der` (REG_BINARY), `Subject`, `Issuer`,
`NotBefore`, `NotAfter`. `Der` se escribe **el último** y luego se hace `flush`,
así que una importación interrumpida deja como mucho una clave sin `Der`:
`anchors()` la ignora y `certctl repair` la borra. El borrado es un
`delete_key` sobre esa clave y un `flush`.

## Permisos

El almacén vive bajo `Software\NeoDOS\Trust`; la modificación está pensada para
usuarios administradores. Hoy los syscalls `cm` son `user()` y **no hay logon
real (USR-P2)**: todo proceso corre con token de administrador, así que la
restricción es de diseño y se aplicará cuando existan sesiones. Queda anotado
como deuda en #753-adyacente.

## Integración

- `libnet::http_transport::{system_trust, system_roots, trust_status}` leen la
  unión de ambos almacenes; `neocurl` no necesita `--ca` (sigue pudiendo
  sustituirlos).
- `--ca`/`-` no cambian: la validación de cadena, hostname y fechas es la misma
  con anclas importadas.
- **NeoCfg**: la sección futura `Security → Certificates` consume
  `libtrust::store::{system_entries, user_entries, import_all, remove}`; el
  módulo de TUI es solo presentación (ver `libneocfg`).
