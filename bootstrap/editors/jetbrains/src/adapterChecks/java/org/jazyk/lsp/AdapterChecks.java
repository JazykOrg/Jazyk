package org.jazyk.lsp;

import java.nio.file.Files;
import java.nio.file.Path;
import java.util.Comparator;
import java.lang.reflect.Proxy;
import com.intellij.openapi.project.Project;
import com.redhat.devtools.lsp4ij.server.OSProcessStreamConnectionProvider;

public final class AdapterChecks {
    private static void check(boolean condition, String message) {
        if (!condition) throw new AssertionError(message);
    }

    public static void main(String[] args) throws Exception {
        Path root = Files.createTempDirectory("jazyk adapter space ");
        try {
            check(JazykDocumentMatcher.projectRoot(root.toString()) == null, "ordinary folders must not activate Jazyk");
            Files.createDirectory(root.resolve(".jazyk"));
            Files.writeString(root.resolve(".jazyk/jazyk.toml"), "");
            Path nested = Files.createDirectories(root.resolve("docs/nested"));
            check(root.equals(JazykDocumentMatcher.projectRoot(nested.toString())), "nested project discovery");
            check(JazykDocumentMatcher.projectRoot(null) == null, "default project");
            check(JazykServerFactory.command(root).equals("jazyk"), "PATH fallback");
            Files.writeString(root.resolve(".jazyk/ide.json"), "{\"lsp\":{\"command\":\"/older/jazyk\"}}");
            check(JazykServerFactory.command(root).equals("/older/jazyk"), "the older location still reads");
            Files.createDirectories(root.resolve(".jazyk/local"));
            Files.writeString(root.resolve(".jazyk/local/ide.json"), "{\"lsp\":{\"command\":\"/path with spaces/jazyk\"}}");
            check(JazykServerFactory.command(root).equals("/path with spaces/jazyk"), "literal executable path");
            Project project = (Project) Proxy.newProxyInstance(Project.class.getClassLoader(),
                new Class<?>[] { Project.class }, (proxy, method, arguments) -> {
                    if (method.getName().equals("getBasePath")) return nested.toString();
                    throw new UnsupportedOperationException(method.getName());
                });
            var provider = (OSProcessStreamConnectionProvider) new JazykServerFactory().createConnectionProvider(project);
            check(provider.getCommands().equals(java.util.List.of("/path with spaces/jazyk", "lsp")), "stdio launch must not split executable paths");
            check(provider.getCommandLine().getWorkDirectory().toPath().equals(root), "server starts in the Jazyk project root");
            Files.writeString(root.resolve(".jazyk/local/ide.json"), "{\"lsp\":{\"command\":\"\"}}");
            boolean failed = false;
            try { JazykServerFactory.command(root); } catch (java.io.IOException expected) { failed = true; }
            check(failed, "empty configured commands must fail clearly");
            System.out.println("Jazyk JetBrains adapter checks passed");
        } finally {
            try (var files = Files.walk(root)) {
                for (Path file : files.sorted(Comparator.reverseOrder()).toList()) Files.delete(file);
            }
        }
    }
}
