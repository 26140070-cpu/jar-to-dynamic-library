# Build a library with GitHub Actions

This guide explains how to use GitHub Actions to build the library without
installing Rust or a JDK on your computer.

## 1. Create a private repository

Create a GitHub account if you do not already have one, then create a private
repository. Keeping it private prevents your mod JAR and build outputs from
being publicly visible.

![Create a repository](images/create-repo-1.png)

Choose a repository name and select **Private**.

![Choose a private repository](images/create-repo-2.png)

## 2. Import this project

Use GitHub's **Import repository** feature to copy this project into your
private repository.

![Start importing](images/import-1.png)

Enter the URL of this repository.

![Enter the repository URL](images/import-2.png)

Wait for the import to finish.

![Wait for the import](images/import-3.png)

Open the imported repository.

![Open the imported repository](images/import-4.png)

## 3. Enable GitHub Actions

Open **Settings**.

![Open Settings](images/allow-gh-actions-1.png)

Select **Actions**.

![Open Actions settings](images/allow-gh-actions-2.png)

Select **General**.

![Open general Actions settings](images/allow-gh-actions-3.png)

Allow the actions used by the repository.

![Allow actions](images/allow-gh-actions-4.png)

Open the **Actions** tab and enable workflows for the repository.

![Open the Actions tab](images/enable-gh-actions-1.png)

![Enable workflows](images/enable-gh-actions-2.png)

## 4. Add your mod JAR

Open the **Code** tab.

![Open the Code tab](images/upload-1.png)

Choose **Add file**.

![Open Add file](images/upload-2.png)

Choose **Upload files**.

![Upload files](images/upload-3.png)

Upload your mod `.jar` file. You do not need to rename it to `input.jar`.

![Choose your JAR file](images/upload-4.png)

Commit the uploaded file to the repository.

![Commit the JAR file](images/upload-5.png)

## 5. Run the build and download your library

Open the **Actions** tab.

![Open Actions](images/upload-6.png)

Select the running build workflow.

![Select the workflow](images/upload-7.png)

Wait for the build to complete.

![Wait for the build](images/upload-8.png)

Download the artifact for your platform from the completed workflow run:

- `jar-to-dynamic-library-windows-x64` contains the `.dll`.
- `jar-to-dynamic-library-linux-x64` contains the `.so`.
- `jar-to-dynamic-library-macos-arm64` contains the Apple Silicon `.dylib`.
- `jar-to-dynamic-library-macos-x64` contains the Intel `.dylib`.

The screenshot below may show an older artifact name.

![Download the library artifact](images/upload-9.png)

Keep the repository private if you do not want your JAR or build artifacts to
be public. The native library must still be loaded into the Minecraft process
separately; this workflow only creates the library.
