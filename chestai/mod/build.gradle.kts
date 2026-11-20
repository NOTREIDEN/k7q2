// The mod for Forge 1.8.9: the game-free core, the code that reads and plays the
// client, and the class Forge loads. Built with Gradle 8.8:  gradle build
// The jar for the mods folder is build/libs/chestai.jar. The policy it carries is
// forge/src/main/resources/chestai/policy.bin, written by python/export_policy.py.
plugins {
    java
    id("gg.essential.loom") version "0.10.0.+"
    id("dev.architectury.architectury-pack200") version "0.1.3"
}

group = "chestai"
version = "1.0"

java {
    toolchain.languageVersion.set(JavaLanguageVersion.of(8))
}

loom {
    forge {
        pack200Provider.set(dev.architectury.pack200.java.Pack200Adapter())
    }
}

sourceSets.main {
    java.setSrcDirs(listOf("core/src/main/java", "client/src/main/java", "forge/src/main/java"))
    resources.setSrcDirs(listOf("forge/src/main/resources"))
}

repositories {
    mavenCentral()
}

dependencies {
    minecraft("com.mojang:minecraft:1.8.9")
    mappings("de.oceanlabs.mcp:mcp_stable:22-1.8.9")
    forge("net.minecraftforge:forge:1.8.9-11.15.1.2318-1.8.9")
}

tasks.withType(JavaCompile::class) {
    options.encoding = "UTF-8"
}

tasks.withType(org.gradle.jvm.tasks.Jar::class) {
    archiveBaseName.set("chestai")
    archiveVersion.set("")
}

// The jar with the names of a workspace, and from it the one with the names of a running game.
tasks.jar {
    archiveClassifier.set("dev")
    destinationDirectory.set(layout.buildDirectory.dir("intermediates"))
}

tasks.named<net.fabricmc.loom.task.RemapJarTask>("remapJar") {
    archiveClassifier.set("")
    dependsOn(tasks.jar)
    input.set(tasks.jar.get().archiveFile)
}

tasks.assemble.get().dependsOn(tasks.named("remapJar"))

// gradle selftest: a Forge client in which the mod tries itself out and which then shuts
// down (see SelfTest). The plugin's own runClient, which does not pass the checks of this
// Gradle, as a plain task.
tasks.register<JavaExec>("selftest") {
    val run = net.fabricmc.loom.configuration.ide.RunConfig.runConfig(project, loom.runConfigs.getByName("client"))
    dependsOn("downloadAssets")
    javaLauncher.set(javaToolchains.launcherFor { languageVersion.set(JavaLanguageVersion.of(8)) })
    classpath = run.sourceSet.runtimeClasspath
    mainClass.set(run.mainClass)
    jvmArgs(run.vmArgs)
    jvmArgs("-Dchestai.selftest=true", "-Dchestai.pegar=si")
    args(run.programArgs)
    workingDir = file(run.runDir)
    environment(run.envVariables)
    doFirst {
        workingDir.mkdirs()
    }
}


// gradle demo: the same client with a small chest farm of its own and the agent at the keys,
// for a video (see Demo). -Pdemo.seconds=120.
tasks.register<JavaExec>("demo") {
    val run = net.fabricmc.loom.configuration.ide.RunConfig.runConfig(project, loom.runConfigs.getByName("client"))
    dependsOn("downloadAssets")
    javaLauncher.set(javaToolchains.launcherFor { languageVersion.set(JavaLanguageVersion.of(8)) })
    classpath = run.sourceSet.runtimeClasspath
    mainClass.set(run.mainClass)
    jvmArgs(run.vmArgs)
    jvmArgs("-Dchestai.demo=true", "-Dchestai.pegar=si", "-Dchestai.debug=true", "-Dchestai.demo.seconds=" + (project.findProperty("demo.seconds") ?: "120"), "-Dchestai.demo.exp=" + (project.findProperty("demo.exp") ?: "false"))
    args(run.programArgs)
    args("--width", "1280", "--height", "720")
    workingDir = file(run.runDir)
    environment(run.envVariables)
    doFirst {
        workingDir.mkdirs()
    }
}

// gradle arena: the same farm on a natural world of a seed, with walls, houses and fences on
// the ways, the agent at the keys and the play measured into run/arena.txt (see Demo).
// -Parena.seed=7 -Parena.seconds=300.
tasks.register<JavaExec>("arena") {
    val run = net.fabricmc.loom.configuration.ide.RunConfig.runConfig(project, loom.runConfigs.getByName("client"))
    dependsOn("downloadAssets")
    javaLauncher.set(javaToolchains.launcherFor { languageVersion.set(JavaLanguageVersion.of(8)) })
    classpath = run.sourceSet.runtimeClasspath
    mainClass.set(run.mainClass)
    jvmArgs(run.vmArgs)
    jvmArgs("-Dchestai.demo=true", "-Dchestai.arena=true", "-Dchestai.pegar=si",
        "-Dchestai.arena.seed=" + (project.findProperty("arena.seed") ?: "1"),
        "-Dchestai.demo.seconds=" + (project.findProperty("arena.seconds") ?: "300"),
        "-Dchestai.arena.video=" + (project.findProperty("arena.video") ?: "false"),
        "-Dchestai.arena.variant=" + (project.findProperty("arena.variant") ?: "base"))
    // -Parena.jvm="-Dchestai.fight.jump=true -Dchestai.fight.walk=true": a variant of the play to compare.
    val extra = (project.findProperty("arena.jvm") as String?)?.trim() ?: ""
    if (extra.isNotEmpty()) {
        jvmArgs(extra.split(Regex("\\s+")))
    }
    args(run.programArgs)
    workingDir = file(run.runDir)
    environment(run.envVariables)
    doFirst {
        workingDir.mkdirs()
    }
}
