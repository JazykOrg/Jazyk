package org.jazyk.lsp;

import com.google.gson.JsonObject;
import com.google.gson.JsonParser;
import com.intellij.execution.configurations.GeneralCommandLine;
import com.intellij.openapi.project.Project;
import com.redhat.devtools.lsp4ij.LanguageServerFactory;
import com.redhat.devtools.lsp4ij.server.OSProcessStreamConnectionProvider;
import com.redhat.devtools.lsp4ij.server.StreamConnectionProvider;
import java.io.IOException;
import java.nio.file.Files;
import java.nio.file.Path;

public final class JazykServerFactory implements LanguageServerFactory {
    static String command(Path root) throws IOException {
        // .jazyk/local/ide.json; .jazyk/ide.json is where an older Jazyk wrote it.
        Path config = root.resolve(".jazyk/local/ide.json");
        if (!Files.exists(config)) config = root.resolve(".jazyk/ide.json");
        if (!Files.exists(config)) return "jazyk";
        JsonObject json = JsonParser.parseString(Files.readString(config)).getAsJsonObject();
        if (!json.has("lsp") || !json.getAsJsonObject("lsp").has("command")) return "jazyk";
        String command = json.getAsJsonObject("lsp").get("command").getAsString();
        if (command.isBlank()) throw new IOException("Empty Jazyk LSP command in " + config);
        return command;
    }

    @Override
    public StreamConnectionProvider createConnectionProvider(Project project) {
        Path root = JazykDocumentMatcher.projectRoot(project.getBasePath());
        if (root == null) throw new IllegalStateException("Open a Jazyk project first");
        try {
            GeneralCommandLine commandLine = new GeneralCommandLine(command(root), "lsp")
                .withWorkDirectory(root.toFile())
                .withParentEnvironmentType(GeneralCommandLine.ParentEnvironmentType.CONSOLE);
            OSProcessStreamConnectionProvider provider = new OSProcessStreamConnectionProvider() {};
            provider.setCommandLine(commandLine);
            return provider;
        } catch (IOException | RuntimeException error) {
            throw new IllegalStateException("Cannot configure Jazyk LSP. Run your `jazyk <ide>` launcher from the project terminal.", error);
        }
    }
}
