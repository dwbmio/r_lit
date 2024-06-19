#!/bin/bash
set -u

OS=$(uname)
BIN_FILE=img_resize
echo "install bin: $BIN_FILE...";

# ==========function 
abort() {
    printf "%s\n" "$@" >&2
    exit 1
}


donwload_bin() {
    echo "downloading..."
    STATIC_DOWN_ADDRESS=http://static.bbclient.icu:8081/${BIN_FILE}/bin/${OS}/${BIN_FILE}
    # download
    curl  -# -o ${BIN_FILE} ${STATIC_DOWN_ADDRESS}

    chmod a+x ${BIN_FILE}
}


install_bin() {
    if [[ "${OS}" == Darwin ]]
    then
        echo "easy install..."
        mv -f ${BIN_FILE} /usr/local/bin
    fi
    echo "install suc!"
    echo "Just run ${BIN_FILE} in command tool to have test."
}



# ==========process 
echo "Current os is ${OS}..."
if [[ "${OS}" == Darwin ]]
then
    donwload_bin
    install_bin
else 
    abort "${BIN_FILE} not support platform ${OS} yet!"
fi

