package middleware

import (
	"bytes"
	"compress/gzip"
	"io"
	"io/fs"
	"mime"
	"net/http"
	"path"
	"strings"
	"time"

	"github.com/bestruirui/octopus/internal/server/resp"
	"github.com/gin-gonic/gin"
)

func StaticEmbed(urlPrefix string, embedFS fs.FS) gin.HandlerFunc {
	return static(urlPrefix, http.FS(embedFS))
}

func StaticLocal(urlPrefix string, localPath string) gin.HandlerFunc {
	return static(urlPrefix, http.Dir(localPath))
}

func openStatic(fileSystem http.FileSystem, name string) http.File {
	file, err := fileSystem.Open(name)
	if err != nil {
		return nil
	}
	stat, err := file.Stat()
	if err != nil || stat.IsDir() {
		file.Close()
		return nil
	}
	return file
}

func static(urlPrefix string, fileSystem http.FileSystem) gin.HandlerFunc {
	return func(c *gin.Context) {
		if strings.HasPrefix(c.Request.URL.Path, "/api") {
			c.Next()
			return
		}
		name := path.Clean("/" + strings.TrimPrefix(c.Request.URL.Path, urlPrefix))
		if name == "/" {
			name = "/index.html"
		}

		acceptsGzip := strings.Contains(c.GetHeader("Accept-Encoding"), "gzip")

		var file http.File
		var encoded, inflate bool
		if acceptsGzip {
			if file = openStatic(fileSystem, name+".gz"); file != nil {
				encoded = true
			}
		}
		if file == nil {
			file = openStatic(fileSystem, name)
		}
		if file == nil && !acceptsGzip {
			if file = openStatic(fileSystem, name+".gz"); file != nil {
				inflate = true
			}
		}
		if file == nil {
			c.Next()
			return
		}
		defer file.Close()

		if strings.HasPrefix(name, "/assets/") {
			c.Header("Cache-Control", "public, max-age=31536000, immutable")
		} else {
			c.Header("Cache-Control", "no-cache")
		}
		if encoded || inflate {
			c.Header("Vary", "Accept-Encoding")
		}

		var content io.ReadSeeker = file
		switch {
		case encoded:
			c.Header("Content-Encoding", "gzip")
			if ctype := mime.TypeByExtension(path.Ext(name)); ctype != "" {
				c.Header("Content-Type", ctype)
			}
		case inflate:
			reader, err := gzip.NewReader(file)
			if err != nil {
				resp.Error(c, http.StatusInternalServerError, resp.ErrInternalServer)
				c.Abort()
				return
			}
			defer reader.Close()
			raw, err := io.ReadAll(reader)
			if err != nil {
				resp.Error(c, http.StatusInternalServerError, resp.ErrInternalServer)
				c.Abort()
				return
			}
			content = bytes.NewReader(raw)
		}
		http.ServeContent(c.Writer, c.Request, name, time.Time{}, content)
		c.Abort()
	}
}
