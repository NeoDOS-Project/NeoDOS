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

## Permisos

Las mutaciones del Registry pasan por una comprobación real en Cm
(`neodos-kernel/src/cm/security.rs`), no por ocultar comandos en `certctl`:

- `\Registry\Machine\Software\NeoDOS\Trust` tiene una DACL: `Everyone`
  (`S-1-1-0`) puede **leer**; solo administradores/SYSTEM pueden crear, escribir
  o borrar.
- La comprobación usa `SeAccessCheck` con la ruta completa de la clave
  (`Hive::key_path`) y se aplica en `cm_create_key`, `cm_set_value`,
  `cm_delete_value` y `cm_delete_key`, incluidas las variantes `ob_set_info`.
- Borrar un **ancestro** del almacén (por ejemplo `Software\NeoDOS`) también
  exige autorización, porque arrastraría `Trust` en el borrado recursivo.

**Bloqueos conocidos:** hoy todo el userland hereda un token de administrador
(#767), así que la política aún no se puede ejercitar end-to-end; y VFS no tiene
ACLs (#42/#43), por lo que un proceso podría escribir `SOFTWARE.hiv` directamente.
La comprobación de Cm es real y quedará efectiva en cuanto existan identidades no
privilegiadas y permisos de fichero. No se simula seguridad en userland.

**Empaquetado de `certctl`:** el listado fijo de binarios del image builder de
NeoDev 0.3.0 todavía no incluye `certctl` (#586); el `.NXE` se compila pero no
aparece en la imagen. `neocfg` (módulo *Certificados*) sí está y usa el mismo
`libtrust::store`. Mientras #586 no se resuelva, la vía CLI no está disponible
en el huésped. El seguimiento de la verificación end-to-end está en #786.

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
