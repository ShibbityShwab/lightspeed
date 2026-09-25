class Lightspeed < Formula
  desc "Zero-cost global network optimizer for multiplayer games"
  homepage "https://github.com/ShibbityShwab/lightspeed"
  version "1.6.12"
  # Custom noncommercial license (LightSpeed-NC-1.0), not an SPDX identifier.
  license :cannot_represent

  on_macos do
    on_intel do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.6.12/lightspeed-client-x86_64-apple-darwin.tar.xz"
      sha256 "1884ae2d62dab4b5e5d094e19dd7cdcce4ce6c337e342f6164de770f9a598edc"
    end
    on_arm do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.6.12/lightspeed-client-aarch64-apple-darwin.tar.xz"
      sha256 "68f416f93eaf9e8d8f50d0d7a35761d46e22725117311bb0d586309c6e10f8e8"
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.6.12/lightspeed-client-x86_64-unknown-linux-gnu.tar.xz"
      sha256 "d0bfab6022b231296e985ff5eee0464ddb0775894d6aa607079f40e1a7a2a81f"
    end
    on_arm do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.6.12/lightspeed-client-aarch64-unknown-linux-gnu.tar.xz"
      sha256 "db5621fe613bcadea64ae8d6d8c7d736e7e9039bd8c476be30183dd56863d1e3"
    end
  end

  def install
    bin.install "lightspeed"
  end

  test do
    assert_match "lightspeed", shell_output("#{bin}/lightspeed --version")
  end
end
