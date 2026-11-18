# Chest AI: mod de Forge 1.8.9

La IA que farmea cofres, entrenada en `chestsim`. Es una red distinta a la de PvP.

## Instalar

1. Forge 1.8.9 (11.15.1.2318 o cualquier 1.8.9).
2. Copiar `chestai-1.8.9.jar` (rama `mod` del repo) a la carpeta `mods`.
3. En el juego, la tecla **J** prende y apaga el bot. Se puede cambiar en Controles, en la sección "Chest AI".

## Qué necesita en la barra

Espada, arco (con Punch), manzanas doradas normales, perlas y pociones splash de curación. El mod las encuentra solas en la barra, en cualquier orden. Las flechas pueden estar en cualquier parte del inventario. Al prender, el chat muestra en qué slot encontró cada cosa.

## Cómo se mueve

- La mira gira como mucho 10° por tick de costado y 7° por tick para arriba o abajo. En el entrenamiento se le cobra por girar de golpe, por temblar (ir y volver) y por dar vueltas en círculo, así que apunta suave y no salta de un mob a otro como un aimbot.
- Pega solo con la espada en la mano: en el simulador un golpe con otro ítem hace 1 de daño, igual que en el juego.
- Se le cobra quedarse quieto sin pegar y alejarse de un cofre con oleada en curso.
- Si un mob va montado (el esqueleto en su caballo), sube la mira al jinete en vez de pegarle al caballo, con los mismos topes de giro.
- Usa el arco con Punch para dispararse y viajar a un cofre que está a más de 12 bloques cuando no hay una oleada cerca.

## Cómo encuentra los cofres

Toma como cofre de farmeo todo cofre que tenga encima un holograma (armor stand con nombre) que diga algo que entiende:

- "Click here to claim the rewards!": se puede reclamar.
- Un mensaje en rojo: hay una oleada en curso.
- "Next Wave: 2 minutes 5 seconds", "45 seconds" o "1:30": el tiempo para la próxima oleada.

Recuerda cada cofre que ve, sin límite de cantidad, y el último tiempo que mostró su holograma, que sigue descontando aunque lo pierda de vista. El mapa puede ser de cualquier tamaño: un cofre a más de 150 bloques lo ve en su dirección real pero a 150 bloques, que es lo más lejos que vio en el entrenamiento. En cada momento elige entre los 8 cofres más cercanos que recuerda. El terreno lo lee en vivo alrededor suyo. Guarda las posiciones por servidor en `config/chestai/cofres2-<servidor>.txt`, así que la próxima vez ya sabe dónde están. Solo anota un cofre cuando su holograma dice algo de una oleada (wave, claim, rewards o el texto de `texto_claim`) y está quieto; los nombres que van sobre los mobs y los carteles de NPCs no cuentan.

La pelea, la espera y el reclamo van siempre igual: con un mob a menos de 8 bloques gira la mira de a poco hacia él (como mucho 10° por tick), camina hacia él y pega solo cuando está a tiro, sin saltar para críticos. Al lado de un cofre cuya oleada falta poco (hasta 90 s) se queda quieto esperando. Un cofre para reclamar a menos de 6 bloques lo mira y le hace clic. Las perlas son solo para ir lejos: nunca con mobs cerca, al lado de un cofre ni mirando al piso.

## Ajustes

`config/chestai/ajustes.txt` se crea la primera vez que se prende el bot, y se lee cada vez que se prende:

- `cps=6-9`: clics por segundo. Cada vez que se prende se sortea un valor de ese rango.
- `ping=auto`: el ping que se le dice a la red. Con auto se lee de la lista TAB.
- `correa=5.5`: cuántos bloques se puede alejar un mob de su cofre antes de que el servidor lo teletransporte de vuelta.
- `perla_espera=15`: segundos de espera del servidor entre dos perlas.
- `texto_claim` y `colores_oleada`: si el servidor usa otro texto o color en el holograma.
- `eleccion=azar`: elige sus acciones como en el entrenamiento (recomendado). Con `mejor`, siempre toma la opción más probable.
- `ignorar=bat`: mobs a ignorar, por nombre de entidad.

Para probar otros pesos sin recompilar, se pone un `policy.bin` (escrito por `python/export_policy.py`) en `config/chestai/`.

## Se apaga solo si

Muere el jugador, cambia el mundo o se abre una pantalla que no es un cofre (chat, menú). Si al reclamar se abre la ventana de un cofre, la cierra y sigue.

## Compilar

Con Gradle 8.8 (no una versión más nueva: con 8.14 el jar sale vacío):

    python python/export_policy.py <checkpoint.pt> mod/forge/src/main/resources/chestai/policy.bin
    cd mod && gradle build          # build/libs/chestai.jar
    xvfb-run gradle selftest        # prueba en un cliente: run/selftest.txt dice "ok"

Las pruebas de `core/src/test` comprueban que la observación en Java es la misma que arma el simulador (`ObsCheck` con un dump de `chestsim/examples/dump.rs`), que la red da lo mismo que PyTorch (`NetCheck`) y que los textos de holograma se leen bien (`HoloCheck`).
