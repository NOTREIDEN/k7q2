// The mod for Forge 1.8.9: the game-free core, the code that reads and plays the
// client, and the class Forge loads. Built with Gradle 8.8:  gradle build
// The jar for the mods folder is build/libs/pvpai.jar. The policy it carries is
// forge/src/main/resources/pvpai/policy.bin, written by python/export_policy.py.
plugins {
    java
    id("gg.essential.loom") version "0.10.0.+"
    id("dev.architectury.architectury-pack200") version "0.1.3"
}

group = "pvpai"
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
    archiveBaseName.set("pvpai")
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
    jvmArgs("-Dpvpai.selftest=true")
    args(run.programArgs)
    workingDir = file(run.runDir)
    environment(run.envVariables)
    doFirst {
        workingDir.mkdirs()
    }
}
