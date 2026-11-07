plugins { java }

group = "org.jazyk"
version = "0.1.0"

// Compile against an installed IDE and LSP4IJ, without downloading another IDE.
val ideHome = providers.gradleProperty("ideHome")
val lsp4ijHome = providers.gradleProperty("lsp4ijHome")
dependencies {
    compileOnly(fileTree(ideHome.map { "$it/lib" }) { include("*.jar", "modules/*.jar") })
    compileOnly(fileTree(lsp4ijHome.map { "$it/lib" }) { include("*.jar") })
}
java {
    sourceCompatibility = JavaVersion.VERSION_21
    targetCompatibility = JavaVersion.VERSION_21
}
tasks.jar {
    archiveFileName.set("jazyk-jetbrains.jar")
    isPreserveFileTimestamps = false
    isReproducibleFileOrder = true
}
tasks.register<Zip>("buildPlugin") {
    dependsOn(tasks.jar)
    archiveFileName.set("jazyk-jetbrains-${project.version}.zip")
    destinationDirectory.set(layout.buildDirectory.dir("distributions"))
    from(tasks.jar) { into("jazyk/lib") }
    isPreserveFileTimestamps = false
    isReproducibleFileOrder = true
}

// These checks exercise project discovery and literal executable resolution.
// They need no test framework, IDE process, network or model calls.
val adapterChecks = sourceSets.create("adapterChecks") {
    compileClasspath += sourceSets.main.get().output + sourceSets.main.get().compileClasspath
    runtimeClasspath += compileClasspath
}
val checkAdapter by tasks.registering(JavaExec::class) {
    dependsOn(tasks.named(adapterChecks.classesTaskName))
    classpath = adapterChecks.runtimeClasspath
    mainClass.set("org.jazyk.lsp.AdapterChecks")
}
tasks.check { dependsOn(checkAdapter) }
