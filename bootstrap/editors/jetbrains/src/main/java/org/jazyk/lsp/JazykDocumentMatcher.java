package org.jazyk.lsp;

import com.intellij.openapi.project.Project;
import com.intellij.openapi.vfs.VirtualFile;
import com.redhat.devtools.lsp4ij.AbstractDocumentMatcher;
import java.nio.file.Files;
import java.nio.file.Path;

public final class JazykDocumentMatcher extends AbstractDocumentMatcher {
    static Path projectRoot(String basePath) {
        if (basePath == null) return null;
        for (Path path = Path.of(basePath); path != null; path = path.getParent()) {
            if (Files.isRegularFile(path.resolve(".jazyk/jazyk.toml"))) return path;
        }
        return null;
    }

    @Override
    public boolean match(VirtualFile file, Project project) {
        Path root = projectRoot(project.getBasePath());
        return root != null && Path.of(file.getPath()).normalize().startsWith(root);
    }
}
