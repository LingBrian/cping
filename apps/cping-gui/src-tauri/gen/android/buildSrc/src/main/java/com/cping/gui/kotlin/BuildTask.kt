import java.io.File
import org.apache.tools.ant.taskdefs.condition.Os
import org.gradle.api.DefaultTask
import org.gradle.api.GradleException
import org.gradle.api.logging.LogLevel
import org.gradle.api.tasks.Input
import org.gradle.api.tasks.TaskAction
import javax.inject.Inject
import org.gradle.process.ExecOperations

abstract class BuildTask : DefaultTask() {
    @get:Inject
    abstract val execOperations: ExecOperations

    @Input
    var rootDirRel: String? = null
    @Input
    var projectDir: String? = null
    @Input
    var target: String? = null
    @Input
    var release: Boolean? = null

    @TaskAction
    fun assemble() {
        // Windows 上 npm 是 npm.cmd / npm.bat 这类脚本，Gradle 的 exec 直接
        // CreateProcess 无法可靠启动它们（本机上 "npm"、"npm.cmd"、"npm.bat" 全部失败）。
        // 统一交给 cmd.exe 解析，既符合 PATHEXT 规则，也不依赖具体脚本形态。
        if (Os.isFamily(Os.FAMILY_WINDOWS)) {
            runTauriCliViaCmd()
        } else {
            runTauriCli("npm")
        }
    }

    fun runTauriCliViaCmd() {
        val rootDirRel = rootDirRel ?: throw GradleException("rootDirRel cannot be null")
        val target = target ?: throw GradleException("target cannot be null")
        val release = release ?: throw GradleException("release cannot be null")

        val cliArgs = mutableListOf(
            "npm", "run", "--", "tauri", "android", "android-studio-script",
        )
        if (logger.isEnabled(LogLevel.DEBUG)) {
            cliArgs.add("-vv")
        } else if (logger.isEnabled(LogLevel.INFO)) {
            cliArgs.add("-v")
        }
        if (release) {
            cliArgs.add("--release")
        }
        cliArgs.add("--target")
        cliArgs.add(target)

        execOperations.exec {
            workingDir(File(projectDir, rootDirRel))
            executable("cmd.exe")
            args("/c", cliArgs.joinToString(" "))
        }.assertNormalExitValue()
    }

    fun runTauriCli(executable: String) {
        val rootDirRel = rootDirRel ?: throw GradleException("rootDirRel cannot be null")
        val target = target ?: throw GradleException("target cannot be null")
        val release = release ?: throw GradleException("release cannot be null")
        val args = listOf("run", "--", "tauri", "android", "android-studio-script");

        execOperations.exec {
            workingDir(File(projectDir, rootDirRel))
            executable(executable)
            args(args)
            if (logger.isEnabled(LogLevel.DEBUG)) {
                args("-vv")
            } else if (logger.isEnabled(LogLevel.INFO)) {
                args("-v")
            }
            if (release) {
                args("--release")
            }
            args(listOf("--target", target))
        }.assertNormalExitValue()
    }
}