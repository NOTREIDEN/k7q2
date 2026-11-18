# Chest AI: mod de Forge 1.8.9

La IA que farmea cofres, entrenada en `chestsim`. Es una red distinta a la de PvP.

## Instalar

1. Forge 1.8.9 (11.15.1.2318 o cualquier 1.8.9).
2. Copiar `chestai-1.8.9.jar` (rama `mod` del repo) a la carpeta `mods`.
3. En el juego, la tecla **J** prende y apaga el bot. Se puede cambiar en Controles, en la sección "Chest AI".

## Qué necesita en la barra

Espada, arco (con Punch), manzanas doradas normales, perlas y pociones splash de curación. El mod las encuentra solas en la barra, en cualquier orden. Las flechas pueden estar en cualquier parte del inventario. Al prender, el chat muestra en qué slot encontró cada cosa.

## Cómo encuentra los cofres

Toma como cofre de farmeo todo cofre que tenga encima un holograma (armor stand con nombre) que diga algo que entiende:

- "Click here to claim the rewards!": se puede reclamar.
- Un mensaje en rojo: hay una oleada en curso.
- "Next Wave: 2 minutes 5 seconds", "45 seconds" o "1:30": el tiempo para la próxima oleada.

Recuerda cada cofre que ve, y el último tiempo que mostró su holograma, aunque después lo pierda de vista. Guarda las posiciones por servidor en `config/chestai/cofres-<servidor>.txt`, así que la próxima vez ya sabe dónde están.

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
